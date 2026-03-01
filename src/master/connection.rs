use crate::{MessageType, SerializableKey};
use crate::encryption::EncryptionManager;
use anyhow::Result;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{RwLock, Mutex};
use tokio::time::Duration;
use crate::config::Config;

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
}

// Structure to hold connection info
#[derive(Clone)]
pub struct ConnectionInfo {
    pub address: String,  // Added address field to identify connections
    pub stream: Arc<Mutex<Option<TcpStream>>>,
    pub encryption_manager: EncryptionManager,
    pub status: Arc<Mutex<ConnectionStatus>>,
    pub key: String,  // Store the key for reconnection
}

impl ConnectionInfo {
    pub fn new(address: String, stream: TcpStream, encryption_manager: EncryptionManager, key: String) -> Self {
        ConnectionInfo {
            address,
            stream: Arc::new(Mutex::new(Some(stream))),
            encryption_manager,
            status: Arc::new(Mutex::new(ConnectionStatus::Connected)),
            key,
        }
    }
}

/// Connection management functionality for MasterClient
impl super::MasterClient {
    /// Connect to a specific slave and store the connection
    pub async fn connect_to_slave(&self, address: String, key: String) -> Result<()> {
        println!("Master attempting to connect to slave at: {}", address);
        
        loop {
            match TcpStream::connect(&address).await {
                Ok(mut stream) => {
                    println!("Connected to slave at {}", address);
                    
                    // Authenticate to the slave
                    let auth_result = self.authenticate_to_slave(&mut stream, &key).await?;
                    
                    if !auth_result {
                        println!("Authentication to slave {} failed", address);
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                    
                    println!("Authentication to slave {} successful", address);
                    
                    // Create encryption manager for this connection
                    let encryption_manager = EncryptionManager::new(&key)?;
                    
                    // Store the connection
                    {
                        let mut connections = self.connections.write().await;
                        
                        // Check if this connection already exists
                        let exists = connections.iter().any(|conn_info| {
                            conn_info.address == address
                        });
                        if !exists {
                            connections.push(ConnectionInfo::new(
                                address.clone(),
                                stream,
                                encryption_manager.clone(),
                                key.clone(),
                            ));
                            println!("Added connection to slave at {}", address);
                        } else {
                            // Update existing connection - we need to move the stream out of the loop
                            let mut stream_option = Some(stream);
                            for conn_info in connections.iter_mut() {
                                if conn_info.address == address {
                                    *conn_info.stream.lock().await = stream_option.take();
                                    *conn_info.status.lock().await = ConnectionStatus::Connected;
                                    break; // Exit the loop after updating
                                }
                            }
                        }
                    }
                    
                    // Keep the connection alive by listening for messages
                    let client_clone = self.clone_for_task();
                    let addr_clone = address.clone();
                    let enc_manager_clone = encryption_manager;
                    
                    tokio::spawn(async move {
                        if let Err(e) = client_clone.handle_slave_connection_loop(addr_clone.clone(), enc_manager_clone).await {
                            eprintln!("Connection to slave {} ended: {}", addr_clone, e);
                            
                            // Mark the connection as disconnected and start reconnection
                            client_clone.mark_disconnected_and_start_reconnection(addr_clone.clone(), key.clone()).await;
                        }
                    });
                    
                    // Exit the loop since we established the connection
                    return Ok(());
                }
                Err(e) => {
                    eprintln!("Failed to connect to slave {}: {}, retrying in 2 seconds...", address, e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    /// Handle an established slave connection loop
    async fn handle_slave_connection_loop(&self, address: String, _encryption_manager: EncryptionManager) -> Result<()> {
        // Just keep the connection alive by periodically checking
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    /// Mark a connection as disconnected and start reconnection thread
    pub async fn mark_disconnected_and_start_reconnection(&self, address: String, key: String) {
        println!("Marked connection to {} as disconnected, starting reconnection", address);

        // Find the connection and mark it as disconnected
        let found_connection = {
            let connections = self.connections.read().await;
            let mut found = false;
            for conn_info in connections.iter() {
                if conn_info.address == address {
                    *conn_info.status.lock().await = ConnectionStatus::Disconnected;
                    *conn_info.stream.lock().await = None;
                    found = true;
                    break;
                }
            }
            found
        };

        if !found_connection {
            println!("Warning: Could not find connection for {} to mark as disconnected", address);
        }

        // Start a background thread to reconnect
        let client_clone = self.clone_for_task();
        let address_clone = address.clone();
        let key_clone = key.clone();
        tokio::spawn(async move {
            loop {
                // Wait 1 second before attempting to reconnect
                println!("Attempting to reconnect to {} in 1 second...", address_clone);
                tokio::time::sleep(Duration::from_secs(1)).await;
                
                match TcpStream::connect(&address_clone).await {
                    Ok(mut stream) => {
                        // Try to authenticate
                        match client_clone.authenticate_to_slave(&mut stream, &key_clone).await {
                            Ok(auth_success) => {
                                if auth_success {
                                    // Reconnection successful, update connection info
                                    let mut connections = client_clone.connections.write().await;
                                    for conn_info in connections.iter_mut() {
                                        if conn_info.address == address_clone {
                                            *conn_info.stream.lock().await = Some(stream);
                                            *conn_info.status.lock().await = ConnectionStatus::Connected;
                                            println!("Reconnected to slave at {}", address_clone);
                                            return; // Exit the reconnection loop
                                        }
                                    }
                                } else {
                                    println!("Reconnection to {} failed: authentication failed", address_clone);
                                }
                            }
                            Err(_) => {
                                println!("Reconnection to {} failed: authentication error", address_clone);
                            }
                        }
                    }
                    Err(e) => {
                        println!("Reconnection to {} failed: connection error: {}", address_clone, e);
                    }
                }
            }
        });
    }

    /// Authenticate to a slave
    pub async fn authenticate_to_slave(&self, stream: &mut TcpStream, key: &str) -> Result<bool> {
        // Send authentication message to slave
        let auth_msg = MessageType::Authenticate {
            key: key.to_string(),
        };
        
        let serialized = serde_json::to_vec(&auth_msg)?;
        let len = serialized.len() as u32;
        
        stream.write_all(&len.to_le_bytes()).await?;
        stream.write_all(&serialized).await?;
        
        // Read response from slave
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let msg_len = u32::from_le_bytes(len_buf) as usize;
        
        let mut msg_buf = vec![0u8; msg_len];
        stream.read_exact(&mut msg_buf).await?;
        
        let response: MessageType = serde_json::from_slice(&msg_buf)?;
        
        match response {
            MessageType::AuthResponse { success } => Ok(success),
            _ => Ok(false),
        }
    }

    /// Check if there are any connected slaves
    pub async fn has_connected_slaves(&self) -> bool {
        let connections = self.connections.read().await;
        for conn_info in connections.iter() {
            if *conn_info.status.lock().await == ConnectionStatus::Connected {
                return true;
            }
        }
        false
    }
}