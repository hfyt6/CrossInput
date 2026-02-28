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
use config::Config;
use encryption::EncryptionManager;

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
    shared_key: String,
    connections: Arc<RwLock<HashMap<String, TcpStream>>>,
    encryption_manager: EncryptionManager,
}

impl LanDataClient {
    /// Create a new client with specified role and shared key
    pub fn new(role: ClientRole, shared_key: String) -> Result<Self> {
        Ok(LanDataClient {
            role,
            shared_key: shared_key.clone(),
            connections: Arc::new(RwLock::new(HashMap::new())),
            encryption_manager: EncryptionManager::new(&shared_key)?,
        })
    }

    /// Start the client based on its role
    pub async fn start(&self, port: u16, _master_addr: Option<&str>) -> Result<()> {
        match self.role {
            ClientRole::Master => self.start_master(port).await,
            ClientRole::Slave => self.start_slave(port).await,  // For slave, just listen on the port
        }
    }

    /// Start master client that connects to slave clients
    async fn start_master(&self, port: u16) -> Result<()> {
        println!("Starting master client on port {}", port);
        println!("Master client listening for slave connections...");
        
        // Listen for incoming connections from slaves
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
        
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    println!("New slave connection from: {}", addr);
                    let client = self.clone_for_task();
                    tokio::spawn(async move {
                        if let Err(e) = client.handle_slave_connection(stream).await {
                            eprintln!("Error handling slave connection: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Error accepting slave connection: {}", e);
                }
            }
        }
    }

    /// Start slave client that connects to master client
    async fn start_slave(&self, port: u16) -> Result<()> {
        println!("Starting slave client...");
        println!("Slave client attempting to connect to master...");
        
        // Connect to master client instead of listening
        let addr = format!("127.0.0.1:{}", port); // Assuming master is on localhost for testing
        loop {
            match TcpStream::connect(&addr).await {
                Ok(stream) => {
                    println!("Connected to master at {}", addr);
                    let client = self.clone_for_task();
                    // Handle communication with master
                    if let Err(e) = client.handle_master_connection(stream).await {
                        eprintln!("Error in communication with master: {}", e);
                    }
                    // Only sleep if connection fails or ends, not during active connection
                }
                Err(e) => {
                    eprintln!("Failed to connect to master: {}, retrying in 2 seconds...", e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            }
            // Sleep briefly before attempting to reconnect
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Handle connection from a slave client (when we are master)
    async fn handle_slave_connection(&self, mut stream: TcpStream) -> Result<()> {
        // Perform authentication - slave should authenticate to master
        let auth_result = self.authenticate_slave(&mut stream).await?;
        
        if !auth_result {
            println!("Slave authentication failed, closing connection");
            return Ok(());
        }

        // Handle data sending after successful authentication
        if let Err(e) = self.handle_master_data_sending(stream).await {
            eprintln!("Error in master communication: {}", e);
        }

        Ok(())
    }

    /// Authenticate a connecting slave (when we are master)
    async fn authenticate_slave(&self, stream: &mut TcpStream) -> Result<bool> {
        // Read authentication message from slave
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let msg_len = u32::from_le_bytes(len_buf) as usize;
        
        let mut msg_buf = vec![0u8; msg_len];
        stream.read_exact(&mut msg_buf).await?;
        
        let auth_request: MessageType = serde_json::from_slice(&msg_buf)?;
        
        match auth_request {
            MessageType::Authenticate { key } => {
                // Check if the key matches our shared key
                let success = key == self.shared_key;
                
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

    /// Handle connection to master client (when we are slave)
    async fn handle_master_connection(&self, mut stream: TcpStream) -> Result<()> {
        // First authenticate to the master
        let auth_result = self.authenticate_to_master(&mut stream).await?;
        
        if !auth_result {
            println!("Authentication to master failed");
            return Ok(());
        }
        
        println!("Authentication to master successful");
        
        // Handle data receiving as slave
        self.handle_master_communication_as_slave(stream).await
    }


    /// Authenticate to the master (called by slave)
    async fn authenticate_to_master(&self, stream: &mut TcpStream) -> Result<bool> {
        // Send authentication message to master
        let auth_msg = MessageType::Authenticate {
            key: self.shared_key.clone(),
        };
        
        let serialized = serde_json::to_vec(&auth_msg)?;
        let len = serialized.len() as u32;
        
        stream.write_all(&len.to_le_bytes()).await?;
        stream.write_all(&serialized).await?;
        
        // Read response from master
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


    /// Handle communication for slave client (receiving data)
    async fn handle_master_communication_as_slave(&self, mut stream: TcpStream) -> Result<()> {
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
                                    let decrypted_payload = self.encryption_manager.decrypt(&payload)?;
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

    /// Authenticate the master client (called by slave)
    async fn authenticate_master_to_slave(&self, stream: &mut TcpStream) -> Result<bool> {
        // Read authentication message from master
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let msg_len = u32::from_le_bytes(len_buf) as usize;
        
        let mut msg_buf = vec![0u8; msg_len];
        stream.read_exact(&mut msg_buf).await?;
        
        let auth_request: MessageType = serde_json::from_slice(&msg_buf)?;
        
        match auth_request {
            MessageType::Authenticate { key } => {
                // Check if the key matches our shared key
                let success = key == self.shared_key;
                
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

    /// Handle data sending for master client
    async fn handle_master_data_sending(&self, mut stream: TcpStream) -> Result<()> {
        // In a real implementation, this would receive data from an application
        // and forward it to the connected slave
        loop {
            // For demonstration, we'll send periodic test data
            let data = format!("Data from master at {:?}", std::time::SystemTime::now()).into_bytes();
            if let Err(e) = self.send_data(&mut stream, data).await {
                eprintln!("Error sending data: {}", e);
                break;
            }
            
            // Send a heartbeat periodically to maintain connection
            tokio::time::sleep(Duration::from_millis(500)).await; // Send data every 0.5 seconds
        }
        Ok(())
    }

    /// Send data to a connected client
    async fn send_data(&self, stream: &mut TcpStream, data: Vec<u8>) -> Result<()> {
        // Encrypt data before sending
        let encrypted_data = self.encryption_manager.encrypt(&data)?;
        
        let message = MessageType::Data {
            payload: encrypted_data,
        };
        
        let serialized = serde_json::to_vec(&message)?;
        let len = serialized.len() as u32;
        
        stream.write_all(&len.to_le_bytes()).await?;
        stream.write_all(&serialized).await?;
        
        Ok(())
    }

    /// Clone client for use in async tasks
    fn clone_for_task(&self) -> Self {
        LanDataClient {
            role: self.role,
            shared_key: self.shared_key.clone(),
            connections: self.connections.clone(),
            encryption_manager: self.encryption_manager.clone(),
        }
    }
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
    
    let client;
    let port: u16;
    let master_addr: Option<String>;
    
    match role {
        ClientRole::Master => {
            // Master client: expects config file path as second argument
            if args.len() < 3 {
                eprintln!("Master client requires config file path as second argument");
                eprintln!("Usage: {} master <config_path>", args[0]);
                std::process::exit(1);
            }
            
            let config_path = &args[2];
            let config = Config::from_file(config_path).context("Failed to load config file")?;
            let shared_key = config.shared_key;
            port = config.port;
            master_addr = None;
            
            client = LanDataClient::new(role, shared_key)?;
        },
        ClientRole::Slave => {
            // Slave client: expects port as second argument and shared key as third argument
            if args.len() < 4 {
                eprintln!("Slave client requires port and shared key as arguments");
                eprintln!("Usage: {} slave <port> <shared_key>", args[0]);
                std::process::exit(1);
            }
            
            port = args[2].parse::<u16>().context("Invalid port number")?;
            let shared_key = args[3].clone(); // Use shared key directly from command line argument
            
            master_addr = None; // Slave will connect to master, so we'll get master address separately
            
            client = LanDataClient::new(role, shared_key)?;
        }
    }
    
    client.start(port, master_addr.as_deref()).await?;
    
    Ok(())
}
