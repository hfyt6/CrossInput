use crate::{MessageType, KeyboardEvent, MouseEvent, MouseButton, SerializableKey};
use crate::encryption::EncryptionManager;
use crate::config::Config;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{RwLock, Mutex};
use tokio::time::Duration;
use std::collections::VecDeque;

// Import the modules we created
mod connection;
mod input_handler;
mod message_queue;
mod data_transmission;

use connection::{ConnectionInfo, ConnectionStatus};

/// Main master client implementation
pub struct MasterClient {
    connections: Arc<RwLock<Vec<ConnectionInfo>>>,
    key_event_queue: Arc<Mutex<VecDeque<MessageType>>>,
}

impl MasterClient {
    /// Create a new master client
    pub fn new() -> Result<Self> {
        Ok(MasterClient {
            connections: Arc::new(RwLock::new(Vec::new())),
            key_event_queue: Arc::new(Mutex::new(VecDeque::new())),
        })
    }

    /// Start the master client
    pub async fn start(&self, config_path: &str) -> Result<()> {
        println!("Starting master client...");
        
        // Load configuration
        let config = Config::from_file(config_path)?;
        
        println!("Master client connecting to {} slave(s)", config.slave_connections.len());
        
        // Store the number of slaves to connect to
        let total_slaves = config.slave_connections.len();

        // Connect to each slave in the configuration
        for slave_conn in &config.slave_connections {
            let client = self.clone_for_task();
            let address = slave_conn.address.clone();
            let key = slave_conn.key.clone();
            let slave_address = slave_conn.address.clone(); // Clone the address for error reporting
            
            tokio::spawn(async move {
                if let Err(e) = client.connect_to_slave(address, key).await {
                    eprintln!("Error connecting to slave {}: {}", slave_address, e);
                }
            });
        }

        // Wait briefly to allow connections to establish
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Check if we have any connections before starting the queue processor
        loop {
            {
                let connections = self.connections.read().await;
                if connections.len() >= total_slaves {
                    println!("All {} slave connections established", connections.len());
                    break;
                }
            }
            println!("Waiting for all slave connections... ({} of {})", 
                     self.connections.read().await.len(), total_slaves);
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Spawn keyboard input listener
        let keyboard_client = self.clone_for_task();
        tokio::spawn(async move {
            if let Err(e) = keyboard_client.listen_for_keyboard_input().await {
                eprintln!("Error in keyboard input listener: {}", e);
            }
        });

        // Start the background thread to poll the message queue
        let queue_client = self.clone_for_task();
        tokio::spawn(async move {
            if let Err(e) = queue_client.process_message_queue().await {
                eprintln!("Error processing message queue: {}", e);
            }
        });

        // Keep the master running
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// Clone client for use in async tasks
    fn clone_for_task(&self) -> Self {
        MasterClient {
            connections: self.connections.clone(),
            key_event_queue: self.key_event_queue.clone(),
        }
    }
}