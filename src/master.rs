use crate::{MessageType, KeyboardEvent, MouseEvent, MouseButton, SerializableKey};
use crate::encryption::EncryptionManager;
use anyhow::Result;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{RwLock, Mutex};
use tokio::time::Duration;
use crate::config::Config;

use std::collections::VecDeque;

// Import rdev for low-level keyboard/mouse event capturing
use rdev::{grab, EventType, Key};

#[derive(Debug, Clone, PartialEq)]
enum ConnectionStatus {
    Connected,
    Disconnected,
}

// Structure to hold connection info
#[derive(Clone)]
struct ConnectionInfo {
    address: String,  // Added address field to identify connections
    stream: Arc<Mutex<Option<TcpStream>>>,
    encryption_manager: EncryptionManager,
    status: Arc<Mutex<ConnectionStatus>>,
    key: String,  // Store the key for reconnection
}

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

    /// Process the message queue in a background thread and send messages to all slaves
    async fn process_message_queue(&self) -> Result<()> {
        println!("Starting message queue processor...");
        
        loop {
            // Process all messages in the queue
            let mut messages_to_process = Vec::new();
            {
                let mut queue = self.key_event_queue.lock().await;
                while let Some(message) = queue.pop_front() {
                    messages_to_process.push(message);
                }
            }
            
            // Send all queued messages to slaves
            for message in messages_to_process {
                println!("Processing message from queue: {:?}", message);
                
                // Send the message to all connected slaves
                if let Err(e) = self.broadcast_to_all_slaves(message).await {
                    eprintln!("Error broadcasting message to slaves: {}", e);
                } else {
                    println!("Message successfully sent to all slaves");
                }
            }
            
            // Sleep briefly before checking the queue again (every 10ms as required)
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Connect to a specific slave and store the connection
    async fn connect_to_slave(&self, address: String, key: String) -> Result<()> {
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
                            connections.push(ConnectionInfo {
                                address: address.clone(),
                                stream: Arc::new(Mutex::new(Some(stream))),
                                encryption_manager: encryption_manager.clone(),
                                status: Arc::new(Mutex::new(ConnectionStatus::Connected)),
                                key: key.clone(),
                            });
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
    async fn mark_disconnected_and_start_reconnection(&self, address: String, key: String) {
        println!("Marked connection to {} as disconnected, starting reconnection", address);

        // Find the connection and mark it as disconnected
        let found_connection = {
            println!("About to acquire read lock on connections for {}", address);
            let connections = self.connections.read().await;
            println!("Acquired read lock on connections for {}", address);
            let mut found = false;
            println!("Iterating through {} connections to find {}", connections.len(), address);
            for (i, conn_info) in connections.iter().enumerate() {
                println!("Checking connection #{}: {} against {}", i, conn_info.address, address);
                if conn_info.address == address {
                    println!("Found matching connection for {}, about to acquire status lock", address);
                    *conn_info.status.lock().await = ConnectionStatus::Disconnected;
                    println!("Set status to Disconnected for {}", address);
                    println!("About to acquire stream lock for {}", address);
                    // *conn_info.stream.lock().await = None;
                    println!("Set stream to None for {}", address);
                    found = true;
                    println!("Found connection and marked as disconnected for {}", address);
                    break;
                } else {
                    println!("Connection #{}: {} does not match {}", i, conn_info.address, address);
                }
            }
            println!("Finished iterating through connections, found: {}", found);
            found
        };

        if !found_connection {
            println!("Warning: Could not find connection for {} to mark as disconnected", address);
        } else {
            println!("Successfully marked connection for {} as disconnected", address);
        }

        // Start a background thread to reconnect
        let client_clone = self.clone_for_task();
        let address_clone = address.clone();
        let key_clone = key.clone();
        println!("About to spawn reconnection task for {}", address);
        tokio::spawn(async move {
            println!("Reconnection task spawned for {}", address_clone);
            loop {
                // Wait 1 second before attempting to reconnect
                println!("Attempting to reconnect to {} in 1 second...", address_clone);
                tokio::time::sleep(Duration::from_secs(1)).await;
                
                match TcpStream::connect(&address_clone).await {
                    Ok(mut stream) => {
                        println!("Successfully connected to {}, attempting authentication", address_clone);
                        // Try to authenticate
                        match client_clone.authenticate_to_slave(&mut stream, &key_clone).await {
                            Ok(auth_success) => {
                                if auth_success {
                                    println!("Authentication successful for {}", address_clone);
                                    // Reconnection successful, update connection info
                                    let mut connections = client_clone.connections.write().await;
                                    for conn_info in connections.iter_mut() {
                                        if conn_info.address == address_clone {
                                            println!("About to acquire stream lock for {} during reconnection", address_clone);
                                            *conn_info.stream.lock().await = Some(stream);
                                            println!("Stream updated for {} during reconnection", address_clone);
                                            println!("About to acquire status lock for {} during reconnection", address_clone);
                                            *conn_info.status.lock().await = ConnectionStatus::Connected;
                                            println!("Status updated to Connected for {} during reconnection", address_clone);
                                            println!("Reconnected to slave at {}", address_clone);
                                            return; // Exit the reconnection loop
                                        }
                                    }
                                    println!("Could not find connection info for {} during reconnection", address_clone);
                                } else {
                                    println!("Reconnection to {} failed: authentication failed", address_clone);
                                }
                            }
                            Err(e) => {
                                println!("Reconnection to {} failed: authentication error: {}", address_clone, e);
                            }
                        }
                    }
                    Err(e) => {
                        println!("Reconnection to {} failed: connection error: {}", address_clone, e);
                    }
                }
            }
        });
        println!("Reconnection task has been spawned for {}", address);
    }

    /// Check if there are any connected slaves
    async fn has_connected_slaves(&self) -> bool {
        let connections = self.connections.read().await;
        for conn_info in connections.iter() {
            if *conn_info.status.lock().await == ConnectionStatus::Connected {
                return true;
            }
        }
        false
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


    /// Clone client for use in async tasks
    fn clone_for_task(&self) -> Self {
        MasterClient {
            connections: self.connections.clone(),
            key_event_queue: self.key_event_queue.clone(),
        }
    }

    /// Listen for keyboard and mouse input and output events with global interception
    async fn listen_for_keyboard_input(&self) -> Result<()> {
        // Use rdev::grab to capture and potentially intercept all keyboard and mouse events globally
        println!("Input listener started. Capturing and intercepting ALL keyboard and mouse events including system shortcuts.");
        
        // Create a clone of self to use in the callback
        let client = self.clone_for_task();
        let running = Arc::new(std::sync::Mutex::new(true));
        let running_clone = running.clone();
        
        // Start the rdev grabber in a separate thread
        std::thread::spawn(move || {
            // Define the callback function for handling events with potential interception
            if let Err(error) = rdev::grab(move |event| {
                // Check if we should stop listening
                if !*running_clone.lock().unwrap() {
                    return Some(event); // Return the event normally to stop the grabber
                }

                // Check if there are any connected slaves before processing the event
                // We need to use a runtime to call async functions from this sync context
                let has_connected = {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        client.has_connected_slaves().await
                    })
                };

                // If no slaves are connected, return the event without intercepting
                if !has_connected {
                    // Clear the message queue when no connections are available
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    rt.block_on(async {
                        let mut queue = client.key_event_queue.lock().await;
                        queue.clear();
                    });
                    return Some(event);
                }

                // Process the event and decide whether to intercept it
                match event.event_type {
                    EventType::KeyPress(key) => {
                        // Format the key event message
                        let key_str = format!("{:?}", key);
                        println!("KeyDown: {} (raw event: {:?})", key_str, event);
                        
                        // Create the keyboard event message using serializable key
                        let keyboard_event = MessageType::Keyboard {
                            event: KeyboardEvent::Press { key: key.into() }
                        };
                        
                        // Add to queue in a non-blocking way using a separate thread
                        let client_clone = client.clone_for_task();
                        std::thread::spawn(move || {
                            // Use a runtime handle to run the async function
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                if let Err(e) = client_clone.add_to_key_event_queue(keyboard_event).await {
                                    eprintln!("Error adding keyboard event to queue: {}", e);
                                }
                            });
                        });
                        
                        // Determine if we should intercept this key press
                        if should_restrict(key) {
                            // Intercept the event by returning None
                            None
                        } else {
                            // Allow the event to pass through by returning Some(event)
                            Some(event)
                        }
                    },
                    EventType::KeyRelease(key) => {
                        // Format the key event message
                        let key_str = format!("{:?}", key);
                        println!("KeyUp: {} (raw event: {:?})", key_str, event);
                        
                        // Create the keyboard event message using serializable key
                        let keyboard_event = MessageType::Keyboard {
                            event: KeyboardEvent::Release { key: key.into() }
                        };
                        
                        // Add to queue in a non-blocking way using a separate thread
                        let client_clone = client.clone_for_task();
                        std::thread::spawn(move || {
                            // Use a runtime handle to run the async function
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                if let Err(e) = client_clone.add_to_key_event_queue(keyboard_event).await {
                                    eprintln!("Error adding keyboard event to queue: {}", e);
                                }
                            });
                        });

                        // Check if the user wants to quit (pressing 'q')
                        if matches!(key, Key::ControlRight) {
                            println!("Quitting keyboard listener...");
                            // Stop the listener by setting running to false
                            *running_clone.lock().unwrap() = false;
                            return Some(event);
                        }
                        
                        // Determine if we should intercept this key release
                        if should_restrict(key) {
                            // Intercept the event by returning None
                            None
                        } else {
                            // Allow the event to pass through by returning Some(event)
                            Some(event)
                        }
                    },
                    EventType::ButtonPress(button) => {
                        // Format the mouse button press event
                        println!("Mouse button press: {:?}", button);
                        
                        // Create the mouse event message
                        let mouse_event = MessageType::Mouse {
                            event: MouseEvent::ButtonPress {
                                button: button.into(),
                            }
                        };
                        
                        // Add to queue in a non-blocking way using a separate thread
                        let client_clone = client.clone_for_task();
                        std::thread::spawn(move || {
                            // Use a runtime handle to run the async function
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                if let Err(e) = client_clone.add_to_key_event_queue(mouse_event).await {
                                    eprintln!("Error adding mouse event to queue: {}", e);
                                }
                            });
                        });
                        
                        // Allow the event to pass through by returning Some(event)
                        Some(event)
                        // None
                    },
                    EventType::ButtonRelease(button) => {
                        // Format the mouse button release event
                        println!("Mouse button release: {:?}", button);
                        
                        // Create the mouse event message
                        let mouse_event = MessageType::Mouse {
                            event: MouseEvent::ButtonRelease {
                                button: button.into(),
                            }
                        };
                        
                        // Add to queue in a non-blocking way using a separate thread
                        let client_clone = client.clone_for_task();
                        std::thread::spawn(move || {
                            // Use a runtime handle to run the async function
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                if let Err(e) = client_clone.add_to_key_event_queue(mouse_event).await {
                                    eprintln!("Error adding mouse event to queue: {}", e);
                                }
                            });
                        });
                        
                        // Allow the event to pass through by returning Some(event)
                        // Some(event)
                        None
                    },
                    EventType::MouseMove { x, y } => {
                        // Format the mouse move event
                        println!("Mouse move: at ({}, {})", x, y);
                        
                        // Create the mouse event message
                        let mouse_event = MessageType::Mouse {
                            event: MouseEvent::Move {
                                x: x as i32,
                                y: y as i32,
                            }
                        };
                        
                        // Add to queue in a non-blocking way using a separate thread
                        let client_clone = client.clone_for_task();
                        std::thread::spawn(move || {
                            // Use a runtime handle to run the async function
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                if let Err(e) = client_clone.add_to_key_event_queue(mouse_event).await {
                                    eprintln!("Error adding mouse event to queue: {}", e);
                                }
                            });
                        });
                        
                        // Allow the event to pass through by returning Some(event)
                        Some(event)
                        // None
                    },
                    EventType::Wheel {
                        delta_x,
                        delta_y,
                    } => {
                        // Format the mouse wheel scroll event
                        println!("Mouse wheel scroll: dx={}, dy={}", delta_x, delta_y);
                        
                        // Create the mouse event message
                        let mouse_event = MessageType::Mouse {
                            event: MouseEvent::Scroll {
                                delta_x: delta_x as i32,
                                delta_y: delta_y as i32,
                            }
                        };
                        
                        // Add to queue in a non-blocking way using a separate thread
                        let client_clone = client.clone_for_task();
                        std::thread::spawn(move || {
                            // Use a runtime handle to run the async function
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async {
                                if let Err(e) = client_clone.add_to_key_event_queue(mouse_event).await {
                                    eprintln!("Error adding mouse event to queue: {}", e);
                                }
                            });
                        });
                        
                        // Allow the event to pass through by returning Some(event)
                        // Some(event)
                        None
                    },
                    _ => {
                        // For other events, allow them to pass through
                        Some(event)
                    }
                }
            }) {
                eprintln!("Error starting rdev grabber: {:?}", error);
            }
        });

        // Keep the async function running while the rdev grabber is active
        loop {
            // Check if the listener should stop
            if !*running.lock().unwrap() {
                break;
            }
            // Sleep briefly to prevent excessive CPU usage
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        println!("Input listener stopped.");
        Ok(())
    }

    /// Add a message to the key event queue
    async fn add_to_key_event_queue(&self, message: MessageType) -> Result<()> {
        println!("Adding message to key event queue: {:?}", message);
        
        // Add the message to the queue
        {
            let mut queue = self.key_event_queue.lock().await;
            queue.push_back(message);
        }
        
        Ok(())
    }

    /// Broadcast data to all connected slaves with appropriate encryption
    async fn broadcast_to_all_slaves(&self, message: MessageType) -> Result<()> {
        println!("Broadcasting message to all connected slaves: {:?}", message);
        // Get all connections
        let connections_vec = {
            let connections = self.connections.read().await;
            connections.clone()
        };

        println!("Found {} connected slaves", connections_vec.len());

        // Iterate through all connections and send the data
        for (index, conn_info) in connections_vec.iter().enumerate() {
            // Only send to connected slaves
            if *conn_info.status.lock().await == ConnectionStatus::Connected {
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
                    println!("Successfully sent message to slave #{}: {:?}", index, message);
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

/// Determines if a key event should be restricted/intercepted
/// Returns true if the key should be intercepted (not passed through to applications)
fn should_restrict(key: Key) -> bool {
    // For now, we'll intercept common system shortcut keys to prevent them from being processed by the local system
    // This ensures that keyboard inputs are only processed by the remote system
    match key {
        // Common system shortcut keys that might interfere with the local system
        Key::Alt | Key::AltGr => true,  // Alt and AltGr keys
        Key::ControlLeft | Key::ControlRight => true,
        Key::Tab => true,  // Alt+Tab, Ctrl+Tab
        Key::Escape => true,  // Alt+Esc, Ctrl+Esc
        Key::F1 | Key::F2 | Key::F3 | Key::F4 | Key::F5 | Key::F6 |
        Key::F7 | Key::F8 | Key::F9 | Key::F10 | Key::F11 | Key::F12 => true,  // Common system function keys
        _ => false,
    }
}
