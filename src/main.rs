use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio::time::{timeout, Duration};

mod client_type;
mod config;
mod encryption;
mod error;

use client_type::ClientRole;
use config::{Config, SlaveConnection};
use encryption::EncryptionManager;

// Define a structure to hold connection info with encryption
struct ConnectionInfo {
    address: String,
    key: String,
    encryption_manager: EncryptionManager,
}

/// Default timeout for connection attempts
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);

/// Maximum delay allowed for data transmission (100ms as specified)
const MAX_DELAY: Duration = Duration::from_millis(100);

/// Message types for communication between master and slave clients
#[derive(Serialize, Deserialize, Debug)]
pub enum MessageType {
    /// Authentication message with shared key
    Authenticate { key: String },
    /// Authentication response
    AuthResponse { success: bool },
    /// Data payload
    Data { payload: Vec<u8> },
    /// Heartbeat to maintain connection
    Heartbeat,
    /// Disconnect notification
    Disconnect,
}

/// Main client implementation
pub struct LanDataClient {
    role: ClientRole,
    connections: Arc<RwLock<HashMap<String, TcpStream>>>,
}

impl LanDataClient {
    /// Create a new client with specified role
    pub fn new(role: ClientRole) -> Result<Self> {
        Ok(LanDataClient {
            role,
            connections: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Start the client based on its role
    pub async fn start(&self, config_path: Option<&str>, port: Option<u16>, key: Option<&str>) -> Result<()> {
        match self.role {
            ClientRole::Master => {
                if let Some(config_path) = config_path {
                    self.start_master(config_path).await
                } else {
                    Err(anyhow::anyhow!("Master client requires config file path"))
                }
            },
            ClientRole::Slave => {
                if let (Some(port), Some(key)) = (port, key) {
                    self.start_slave(port, key).await
                } else {
                    Err(anyhow::anyhow!("Slave client requires port and key"))
                }
            }
        }
    }

    /// Start master client that connects to slave clients
    async fn start_master(&self, config_path: &str) -> Result<()> {
        println!("Starting master client...");
        
        // Load configuration
        let config = Config::from_file(config_path).context("Failed to load config file")?;
        
        println!("Master client connecting to {} slave(s)", config.slave_connections.len());
        
        // Connect to each slave in the configuration
        for slave_conn in config.slave_connections {
            let client = self.clone_for_task();
            let address = slave_conn.address.clone();
            let key = slave_conn.key.clone();
            
            tokio::spawn(async move {
                if let Err(e) = client.connect_to_slave(address, key).await {
                    eprintln!("Error connecting to slave {}: {}", slave_conn.address, e);
                }
            });
        }
        
        // Keep the master running
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

/// Connect to a specific slave
    async fn connect_to_slave(&self, address: String, key: String) -> Result<()> {
        println!("Master attempting to connect to slave at: {}", address);
        
        // Create encryption manager for this connection
        let encryption_manager = EncryptionManager::new(&key)?;
        
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
                    
                    // Handle communication with slave
                    if let Err(e) = self.handle_slave_communication_with_encryption(stream, encryption_manager.clone()).await {
                        eprintln!("Error in communication with slave {}: {}", address, e);
                    }
                    
                    // Reconnect after a brief delay if connection ends
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                Err(e) => {
                    eprintln!("Failed to connect to slave {}: {}, retrying in 2 seconds...", address, e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    /// Authenticate to a slave
    async fn authenticate_to_slave(&self, stream: &mut TcpStream, key: &str) -> Result<bool> {
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

/// Handle communication with a slave (sending data)
    async fn handle_slave_communication_with_encryption(&self, mut stream: TcpStream, encryption_manager: EncryptionManager) -> Result<()> {
        // In a real implementation, this would receive data from an application
        // and forward it to the connected slave
        loop {
            // For demonstration, we'll send periodic test data
            let data = format!("Data from master at {:?}", std::time::SystemTime::now()).into_bytes();
            if let Err(e) = send_data_to_stream_with_encryption(&mut stream, data, &encryption_manager).await {
                eprintln!("Error sending data: {}", e);
                break;
            }
            
            // Send a heartbeat periodically to maintain connection
            tokio::time::sleep(Duration::from_millis(500)).await; // Send data every 0.5 seconds
        }
        Ok(())
    }

    /// Start slave client that listens for connections from master
    async fn start_slave(&self, port: u16, key: &str) -> Result<()> {
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

    /// Clone client for use in async tasks
    fn clone_for_task(&self) -> Self {
        LanDataClient {
            role: self.role,
            connections: self.connections.clone(),
        }
    }
}

/// Handle connection from master client (when we are slave)
async fn handle_master_connection_from_slave(mut stream: TcpStream, key: String) -> Result<()> {
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
async fn handle_slave_data_receiving(mut stream: TcpStream, encryption_manager: EncryptionManager) -> Result<()> {
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

/// Send data to a stream with encryption
async fn send_data_to_stream_with_encryption(stream: &mut TcpStream, data: Vec<u8>, encryption_manager: &EncryptionManager) -> Result<()> {
    // Encrypt data before sending
    let encrypted_data = encryption_manager.encrypt(&data)?;
    
    let message = MessageType::Data {
        payload: encrypted_data,
    };
    
    let serialized = serde_json::to_vec(&message)?;
    let len = serialized.len() as u32;
    
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&serialized).await?;
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Example usage:
    // Parse command line arguments to determine role and settings
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage:");
        eprintln!("  For master: {} master <config_path>", args[0]);
        eprintln!("  For slave: {} slave <port> <shared_key>", args[0]);
        eprintln!("Example: {} master ./config.json", args[0]);
        eprintln!("Example: {} slave 8081 mysecretkey", args[0]);
        std::process::exit(1);
    }
    
    let role_str = &args[1];
    let role = match role_str.as_str() {
        "master" => ClientRole::Master,
        "slave" => ClientRole::Slave,
        _ => {
            eprintln!("Invalid role: {}. Use 'master' or 'slave'", role_str);
            std::process::exit(1);
        }
    };
    
    let client = LanDataClient::new(role)?;
    
    match role {
        ClientRole::Master => {
            // Master client: expects config file path as second argument
            if args.len() < 3 {
                eprintln!("Master client requires config file path as second argument");
                eprintln!("Usage: {} master <config_path>", args[0]);
                std::process::exit(1);
            }
            
            let config_path = &args[2];
            client.start(Some(config_path), None, None).await?;
        },
        ClientRole::Slave => {
            // Slave client: expects port as second argument and shared key as third argument
            if args.len() < 4 {
                eprintln!("Slave client requires port and shared key as arguments");
                eprintln!("Usage: {} slave <port> <shared_key>", args[0]);
                std::process::exit(1);
            }
            
            let port = args[2].parse::<u16>().context("Invalid port number")?;
            let key = &args[3]; // Use shared key directly from command line argument
            
            client.start(None, Some(port), Some(key)).await?;
        }
    }
    
    Ok(())
}
