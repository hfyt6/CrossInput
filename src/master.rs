use crate::{MessageType};
use crate::encryption::EncryptionManager;
use crate::client_type::ClientRole;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio::time::Duration;
use crate::config::Config;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;

/// Main master client implementation
pub struct MasterClient {
    connections: Arc<RwLock<HashMap<String, TcpStream>>>,
}

impl MasterClient {
    /// Create a new master client
    pub fn new() -> Result<Self> {
        Ok(MasterClient {
            connections: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Start the master client
    pub async fn start(&self, config_path: &str) -> Result<()> {
        println!("Starting master client...");
        
        // Load configuration
        let config = Config::from_file(config_path)?;
        
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

        // Spawn keyboard input listener
        let keyboard_client = self.clone_for_task();
        tokio::spawn(async move {
            if let Err(e) = keyboard_client.listen_for_keyboard_input().await {
                eprintln!("Error in keyboard input listener: {}", e);
            }
        });
        
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

    /// Clone client for use in async tasks
    fn clone_for_task(&self) -> Self {
        MasterClient {
            connections: self.connections.clone(),
        }
    }

    /// Listen for keyboard input and output events
    async fn listen_for_keyboard_input(&self) -> Result<()> {
        println!("Keyboard input listener started. Press 'a' or 'b' to see events.");
        println!("Press 'q' to quit.");
        
        loop {
            if event::poll(std::time::Duration::from_millis(50)).unwrap() {
                if let Event::Key(KeyEvent { code, modifiers, kind, .. }) = event::read().unwrap() {
                    match code {
                        KeyCode::Char('a') | KeyCode::Char('b') => {
                            let key_char = match code {
                                KeyCode::Char(c) => c,
                                _ => unreachable!(),
                            };
                            
                            match kind {
                                event::KeyEventKind::Press => {
                                    println!("KeyDown: '{}'", key_char);
                                },
                                event::KeyEventKind::Release => {
                                    println!("KeyUp: '{}'", key_char);
                                },
                                event::KeyEventKind::Repeat => {
                                    // Optional: handle repeat events if needed
                                },
                            }
                        },
                        KeyCode::Char('q') => {
                            if kind == event::KeyEventKind::Press {
                                println!("Quitting keyboard listener...");
                                break;
                            }
                        },
                        _ => {
                            // Ignore other keys
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Send data to a stream with encryption
pub async fn send_data_to_stream_with_encryption(stream: &mut TcpStream, data: Vec<u8>, encryption_manager: &EncryptionManager) -> Result<()> {
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