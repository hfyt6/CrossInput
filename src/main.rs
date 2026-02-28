use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

mod client_type;
mod config;
mod encryption;
mod error;
mod master;
mod slave;

use client_type::ClientRole;
use master::MasterClient;
use slave::SlaveClient;

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
pub struct LanDataClient {}

impl LanDataClient {
    /// Create a new client
    pub fn new() -> Result<Self> {
        Ok(LanDataClient {})
    }

    /// Start the client based on its role
    pub async fn start(&self, config_path: Option<&str>, port: Option<u16>, key: Option<&str>) -> Result<()> {
        match (config_path, port, key) {
            (Some(config_path), None, None) => {
                // Master client: expects config file path
                let master = MasterClient::new()?;
                master.start(config_path).await
            },
            (None, Some(port), Some(key)) => {
                // Slave client: expects port and key
                let slave = SlaveClient::new()?;
                slave.start(port, key).await
            },
            _ => {
                Err(anyhow::anyhow!("Invalid arguments: provide either config file path for master or port and key for slave"))
            }
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
    
    let client = LanDataClient::new()?;
    
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
