use crate::{MessageType, SerializableKey};
use crate::encryption::EncryptionManager;
use anyhow::Result;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::{RwLock, Mutex};

/// Data transmission functionality for MasterClient
impl super::MasterClient {
    /// Broadcast data to all connected slaves with appropriate encryption
    pub async fn broadcast_to_all_slaves(&self, message: MessageType) -> Result<()> {
        // println!("Broadcasting message to all connected slaves: {:?}", message);
        // Get all connections
        let connections_vec = {
            let connections = self.connections.read().await;
            connections.clone()
        };

        // println!("Found {} connected slaves", connections_vec.len());

        // Iterate through all connections and send the data
        for (index, conn_info) in connections_vec.iter().enumerate() {
            // Only send to connected slaves
            if *conn_info.status.lock().await == super::connection::ConnectionStatus::Connected {
                println!("Sending message to slave #{}: {:?}", index, message);
                // Lock the stream temporarily to send data
                let mut stream_lock = conn_info.stream.lock().await;
                
                // Encrypt the data with the specific encryption manager for this connection
                if let Err(e) = send_data_to_stream_with_encryption(&mut *stream_lock, message.clone(), &conn_info.encryption_manager).await {
                    eprintln!("Error sending data to slave #{}: {}", index, e);
                    println!("About to call mark_disconnected_and_start_reconnect for {}", conn_info.address);
                    // Mark the connection as disconnected and start reconnection
                    self.mark_disconnected_and_start_reconnection(conn_info.address.clone(), conn_info.key.clone()).await;
                    println!("Called mark_disconnected_and_start_reconnect for {}", conn_info.address);
                } else {
                    // println!("Successfully sent message to slave #{}: {:?}", index, message);
                }
            } else {
                println!("Skipping disconnected slave #{}: {:?}", index, message);
            }
        }
        Ok(())
    }
}

/// Send data to a stream with encryption
pub async fn send_data_to_stream_with_encryption(stream: &mut Option<TcpStream>, message: MessageType, encryption_manager: &EncryptionManager) -> Result<()> {
    if let Some(ref mut tcp_stream) = stream {
        // Encrypt data before sending based on message type
        let message_to_send = match &message {
            MessageType::Keyboard { .. } | MessageType::Mouse { .. } => {
                // For keyboard and mouse events, serialize and encrypt the entire message
                let serialized = serde_json::to_vec(&message)?;
                let encrypted_data = encryption_manager.encrypt(&serialized)?;
                MessageType::Data {
                    payload: encrypted_data,
                }
            },
            _ => message, // For other message types, send as-is
        };
        
        let serialized = serde_json::to_vec(&message_to_send)?;
        let len = serialized.len() as u32;
        
        tcp_stream.write_all(&len.to_le_bytes()).await?;
        tcp_stream.write_all(&serialized).await?;
    } else {
        return Err(anyhow::anyhow!("Stream is disconnected"));
    }
    
    Ok(())
}