use crate::{MessageType};
use crate::encryption::EncryptionManager;
use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration};

/// Maximum delay allowed for data transmission (100ms as specified)
const MAX_DELAY: Duration = Duration::from_millis(100);

/// Main slave client implementation
pub struct SlaveClient {}

impl SlaveClient {
    /// Create a new slave client
    pub fn new() -> Result<Self> {
        Ok(SlaveClient {})
    }

    /// Start the slave client
    pub async fn start(&self, port: u16, key: &str) -> Result<()> {
        println!("Starting slave client on port {}...", port);
        
        // Listen for incoming connections from master
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
        println!("Slave client listening on port {} for master connections...", port);
        
        loop {
            match listener.accept().await {
                Ok((mut stream, addr)) => {
                    println!("New connection from: {}", addr);
                    let key_clone = key.to_string();
                    tokio::spawn(async move {
                        if let Err(e) = handle_master_connection_from_slave(stream, key_clone).await {
                            eprintln!("Error handling master connection: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Error accepting connection: {}", e);
                }
            }
        }
    }
}

/// Handle connection from master client (when we are slave)
pub async fn handle_master_connection_from_slave(mut stream: TcpStream, key: String) -> Result<()> {
    // Perform authentication - master should authenticate to slave
    let auth_result = authenticate_master_from_slave(&mut stream, &key).await?;
    
    if !auth_result {
        println!("Master authentication failed, closing connection");
        return Ok(());
    }

    println!("Master authentication successful");

    // Create encryption manager for this connection
    let encryption_manager = EncryptionManager::new(&key)?;

    // Handle data receiving after successful authentication
    handle_slave_data_receiving(stream, encryption_manager).await
}

/// Authenticate a connecting master (when we are slave)
async fn authenticate_master_from_slave(stream: &mut TcpStream, expected_key: &str) -> Result<bool> {
    // Read authentication message from master
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let msg_len = u32::from_le_bytes(len_buf) as usize;
    
    let mut msg_buf = vec![0u8; msg_len];
    stream.read_exact(&mut msg_buf).await?;
    
    let auth_request: MessageType = serde_json::from_slice(&msg_buf)?;
    
    match auth_request {
        MessageType::Authenticate { key } => {
            // Check if the key matches the expected key for this slave
            let success = key == expected_key;
            
            // Send authentication response
            let response = MessageType::AuthResponse { success };
            let serialized = serde_json::to_vec(&response)?;
            let len = serialized.len() as u32;
            
            stream.write_all(&len.to_le_bytes()).await?;
            stream.write_all(&serialized).await?;
            
            Ok(success)
        }
        _ => Ok(false),
    }
}

/// Handle data receiving for slave client
pub async fn handle_slave_data_receiving(mut stream: TcpStream, encryption_manager: EncryptionManager) -> Result<()> {
    // Now receive data from master - authentication already happened
    println!("Ready to receive data from master");
    loop {
        // Read message length
        let mut len_buf = [0u8; 4];
        let result = timeout(MAX_DELAY, stream.read_exact(&mut len_buf)).await;
        
        match result {
            Ok(Ok(_)) => {
                let msg_len = u32::from_le_bytes(len_buf) as usize;
                
                // Read message content
                let mut msg_buf = vec![0u8; msg_len];
                let content_result = timeout(MAX_DELAY, stream.read_exact(&mut msg_buf)).await;
                
                match content_result {
                    Ok(Ok(_)) => {
                        let message: MessageType = serde_json::from_slice(&msg_buf)?;
                        
                        match message {
                            MessageType::Data { payload } => {
                                // Decrypt payload if encrypted
                                let decrypted_payload = encryption_manager.decrypt(&payload)?;
                                println!("Received data: {:?}", String::from_utf8_lossy(&decrypted_payload));
                            }
                            MessageType::Heartbeat => {
                                println!("Received heartbeat");
                            }
                            MessageType::Disconnect => {
                                println!("Received disconnect signal");
                                break;
                            }
                            _ => {
                                // Ignore other message types
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        // Stream read error - connection likely closed
                        eprintln!("Error reading message content: {}", e);
                        break;
                    }
                    Err(_) => {
                        // Timeout occurred - this is expected behavior, continue loop
                        continue;
                    }
                }
            }
            Ok(Err(e)) => {
                // Stream read error - connection likely closed
                eprintln!("Error reading message length: {}", e);
                break;
            }
            Err(_) => {
                // Timeout occurred - this is expected behavior, continue loop
                continue;
            }
        }
    }
    
    Ok(())
}