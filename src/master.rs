use crate::{MessageType};
use crate::encryption::EncryptionManager;
use anyhow::Result;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{RwLock, Mutex};
use tokio::time::Duration;
use crate::config::Config;
use crossterm::event::{self, Event, KeyCode, KeyEvent};

use std::collections::VecDeque;

#[cfg(target_os = "windows")]
use winapi::um::winuser::{GetAsyncKeyState, VK_SHIFT, VK_CONTROL, VK_MENU, VK_LSHIFT, VK_RSHIFT, VK_LCONTROL, VK_RCONTROL, VK_LMENU, VK_RMENU};
#[cfg(target_os = "windows")]
use std::thread;
#[cfg(target_os = "windows")]
use std::time::Duration as StdDuration;
#[cfg(target_os = "windows")]
use tokio::runtime::Handle;

// Structure to hold connection info
#[derive(Clone)]
struct ConnectionInfo {
    address: String,  // Added address field to identify connections
    stream: Arc<Mutex<TcpStream>>,
    encryption_manager: EncryptionManager,
}

/// Main master client implementation
pub struct MasterClient {
    connections: Arc<RwLock<Vec<ConnectionInfo>>>,
    key_event_queue: Arc<Mutex<VecDeque<String>>>,
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
                println!("Processing message from queue: {}", message);
                
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
                                stream: Arc::new(Mutex::new(stream)),
                                encryption_manager: encryption_manager.clone(),
                            });
                            println!("Added connection to slave at {}", address);
                        } else {
                            // Update existing connection
                            for conn_info in connections.iter_mut() {
                                // In this case, we're just adding new connections
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
                            
                            // Remove the connection from the list
                            let mut connections = client_clone.connections.write().await;
                            connections.retain(|conn_info| conn_info.address != addr_clone);
                            println!("Removed connection to slave at {}", addr_clone);
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
    async fn handle_slave_connection_loop(&self, _address: String, _encryption_manager: EncryptionManager) -> Result<()> {
        // Just keep the connection alive by periodically checking
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
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


    /// Clone client for use in async tasks
    fn clone_for_task(&self) -> Self {
        MasterClient {
            connections: self.connections.clone(),
            key_event_queue: self.key_event_queue.clone(),
        }
    }

    /// Listen for keyboard input and output events
    async fn listen_for_keyboard_input(&self) -> Result<()> {
        #[cfg(not(target_os = "windows"))]
        {
            // For non-Windows platforms, use crossterm
            println!("Keyboard input listener started. Capturing all keyboard events.");
            println!("Press 'q' to quit.");
            
            loop {
                if event::poll(std::time::Duration::from_millis(50)).unwrap() {
                    if let Event::Key(key_event) = event::read().unwrap() {
                        let KeyEvent { code, modifiers, kind, state: _ } = key_event;
                        
                        // Capture all keyboard events, including modifier keys and combinations
                        match code {
                            KeyCode::Char('q') => {
                                if kind == event::KeyEventKind::Press {
                                    println!("Quitting keyboard listener...");
                                    break;
                                }
                            },
                            KeyCode::Char(c) => {
                                let key_char = c;
                                
                                match kind {
                                    event::KeyEventKind::Press => {
                                        println!("KeyDown: '{}' (modifiers: {:?})", key_char, modifiers);
                                        // Add message to the key event queue
                                        let event_message = format!("KeyDown: '{}' (modifiers: {:?})", key_char, modifiers);
                                        self.add_to_key_event_queue(event_message).await?;
                                        println!("Event added to queue: KeyDown '{}'", key_char);
                                    },
                                    event::KeyEventKind::Release => {
                                        println!("KeyUp: '{}' (modifiers: {:?})", key_char, modifiers);
                                        // Add message to the key event queue
                                        let event_message = format!("KeyUp: '{}' (modifiers: {:?})", key_char, modifiers);
                                        self.add_to_key_event_queue(event_message).await?;
                                        println!("Event added to queue: KeyUp '{}'", key_char);
                                    },
                                    event::KeyEventKind::Repeat => {
                                        println!("KeyRepeat: '{}' (modifiers: {:?})", key_char, modifiers);
                                        // Add message to the key event queue
                                        let event_message = format!("KeyRepeat: '{}' (modifiers: {:?})", key_char, modifiers);
                                        self.add_to_key_event_queue(event_message).await?;
                                        println!("Event added to queue: KeyRepeat '{}'", key_char);
                                    },
                                }
                            },
                            // Handle special keys
                            _ => {
                                // Format special keys appropriately
                                let key_name = match code {
                                    KeyCode::Backspace => "Backspace",
                                    KeyCode::Enter => "Enter",
                                    KeyCode::Left => "ArrowLeft",
                                    KeyCode::Right => "ArrowRight",
                                    KeyCode::Up => "ArrowUp",
                                    KeyCode::Down => "ArrowDown",
                                    KeyCode::Home => "Home",
                                    KeyCode::End => "End",
                                    KeyCode::PageUp => "PageUp",
                                    KeyCode::PageDown => "PageDown",
                                    KeyCode::Tab => "Tab",
                                    KeyCode::Delete => "Delete",
                                    KeyCode::Insert => "Insert",
                                    KeyCode::F(1) => "F1",
                                    KeyCode::F(2) => "F2",
                                    KeyCode::F(3) => "F3",
                                    KeyCode::F(4) => "F4",
                                    KeyCode::F(5) => "F5",
                                    KeyCode::F(6) => "F6",
                                    KeyCode::F(7) => "F7",
                                    KeyCode::F(8) => "F8",
                                    KeyCode::F(9) => "F9",
                                    KeyCode::F(10) => "F10",
                                    KeyCode::F(11) => "F11",
                                    KeyCode::F(12) => "F12",
                                    KeyCode::Esc => "Escape",
                                    KeyCode::CapsLock => "CapsLock",
                                    KeyCode::ScrollLock => "ScrollLock",
                                    KeyCode::NumLock => "NumLock",
                                    KeyCode::PrintScreen => "PrintScreen",
                                    KeyCode::Pause => "Pause",
                                    KeyCode::Menu => "Menu",
                                    KeyCode::KeypadBegin => "KeypadBegin",
                                    KeyCode::Media(_) => "Media",
                                    KeyCode::Modifier(_) => "Modifier",
                                    _ => "Unknown",
                                };
                                
                                match kind {
                                    event::KeyEventKind::Press => {
                                        println!("KeyDown: {} (modifiers: {:?})", key_name, modifiers);
                                        // Add message to the key event queue
                                        let event_message = format!("KeyDown: {} (modifiers: {:?})", key_name, modifiers);
                                        self.add_to_key_event_queue(event_message).await?;
                                        println!("Event added to queue: KeyDown {}", key_name);
                                    },
                                    event::KeyEventKind::Release => {
                                        println!("KeyUp: {} (modifiers: {:?})", key_name, modifiers);
                                        // Add message to the key event queue
                                        let event_message = format!("KeyUp: {} (modifiers: {:?})", key_name, modifiers);
                                        self.add_to_key_event_queue(event_message).await?;
                                        println!("Event added to queue: KeyUp {}", key_name);
                                    },
                                    event::KeyEventKind::Repeat => {
                                        println!("KeyRepeat: {} (modifiers: {:?})", key_name, modifiers);
                                        // Add message to the key event queue
                                        let event_message = format!("KeyRepeat: {} (modifiers: {:?})", key_name, modifiers);
                                        self.add_to_key_event_queue(event_message).await?;
                                        println!("Event added to queue: KeyRepeat {}", key_name);
                                    },
                                }
                            }
                        }
                    }
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            // For Windows, use Windows API to capture all keyboard events including modifier keys
            println!("Windows keyboard input listener started. Capturing all keyboard events including modifier keys.");
            println!("Press 'q' to quit.");
            
            // Keep track of the state of modifier keys to detect press/release events
            let mut shift_pressed = false;
            let mut ctrl_pressed = false;
            let mut alt_pressed = false;
            
            loop {
                // Check for regular character input using crossterm
                if event::poll(std::time::Duration::from_millis(10)).unwrap() {
                    if let Event::Key(key_event) = event::read().unwrap() {
                        let KeyEvent { code, modifiers, kind, state: _ } = key_event;
                        
                        match code {
                            KeyCode::Char('q') => {
                                if kind == event::KeyEventKind::Press {
                                    println!("Quitting keyboard listener...");
                                    break;
                                }
                            },
                            KeyCode::Char(c) => {
                                match kind {
                                    event::KeyEventKind::Press => {
                                        println!("KeyDown: '{}' (modifiers: {:?})", c, modifiers);
                                        let event_message = format!("KeyDown: '{}' (modifiers: {:?})", c, modifiers);
                                        self.add_to_key_event_queue(event_message).await?;
                                    },
                                    event::KeyEventKind::Release => {
                                        println!("KeyUp: '{}' (modifiers: {:?})", c, modifiers);
                                        let event_message = format!("KeyUp: '{}' (modifiers: {:?})", c, modifiers);
                                        self.add_to_key_event_queue(event_message).await?;
                                    },
                                    event::KeyEventKind::Repeat => {
                                        println!("KeyRepeat: '{}' (modifiers: {:?})", c, modifiers);
                                        let event_message = format!("KeyRepeat: '{}' (modifiers: {:?})", c, modifiers);
                                        self.add_to_key_event_queue(event_message).await?;
                                    },
                                }
                            },
                            _ => {
                                // Handle special keys that crossterm captures
                                let key_name = match code {
                                    KeyCode::Backspace => "Backspace",
                                    KeyCode::Enter => "Enter",
                                    KeyCode::Left => "ArrowLeft",
                                    KeyCode::Right => "ArrowRight",
                                    KeyCode::Up => "ArrowUp",
                                    KeyCode::Down => "ArrowDown",
                                    KeyCode::Home => "Home",
                                    KeyCode::End => "End",
                                    KeyCode::PageUp => "PageUp",
                                    KeyCode::PageDown => "PageDown",
                                    KeyCode::Tab => "Tab",
                                    KeyCode::Delete => "Delete",
                                    KeyCode::Insert => "Insert",
                                    KeyCode::F(1) => "F1",
                                    KeyCode::F(2) => "F2",
                                    KeyCode::F(3) => "F3",
                                    KeyCode::F(4) => "F4",
                                    KeyCode::F(5) => "F5",
                                    KeyCode::F(6) => "F6",
                                    KeyCode::F(7) => "F7",
                                    KeyCode::F(8) => "F8",
                                    KeyCode::F(9) => "F9",
                                    KeyCode::F(10) => "F10",
                                    KeyCode::F(11) => "F11",
                                    KeyCode::F(12) => "F12",
                                    KeyCode::Esc => "Escape",
                                    KeyCode::CapsLock => "CapsLock",
                                    KeyCode::ScrollLock => "ScrollLock",
                                    KeyCode::NumLock => "NumLock",
                                    KeyCode::PrintScreen => "PrintScreen",
                                    KeyCode::Pause => "Pause",
                                    KeyCode::Menu => "Menu",
                                    _ => "SpecialKey",
                                };
                                
                                match kind {
                                    event::KeyEventKind::Press => {
                                        println!("KeyDown: {} (modifiers: {:?})", key_name, modifiers);
                                        let event_message = format!("KeyDown: {} (modifiers: {:?})", key_name, modifiers);
                                        self.add_to_key_event_queue(event_message).await?;
                                    },
                                    event::KeyEventKind::Release => {
                                        println!("KeyUp: {} (modifiers: {:?})", key_name, modifiers);
                                        let event_message = format!("KeyUp: {} (modifiers: {:?})", key_name, modifiers);
                                        self.add_to_key_event_queue(event_message).await?;
                                    },
                                    event::KeyEventKind::Repeat => {
                                        println!("KeyRepeat: {} (modifiers: {:?})", key_name, modifiers);
                                        let event_message = format!("KeyRepeat: {} (modifiers: {:?})", key_name, modifiers);
                                        self.add_to_key_event_queue(event_message).await?;
                                    },
                                }
                            }
                        }
                    }
                }

                // Check modifier keys using Windows API
                unsafe {
                    // Check Shift key states (both left and right)
                    let lshift_state = GetAsyncKeyState(VK_LSHIFT) as u16;
                    let rshift_state = GetAsyncKeyState(VK_RSHIFT) as u16;
                    let shift_down = (lshift_state & 0x8000) != 0 || (rshift_state & 0x8000) != 0;
                    
                    // Check Ctrl key states (both left and right)
                    let lctrl_state = GetAsyncKeyState(VK_LCONTROL) as u16;
                    let rctrl_state = GetAsyncKeyState(VK_RCONTROL) as u16;
                    let ctrl_down = (lctrl_state & 0x8000) != 0 || (rctrl_state & 0x8000) != 0;
                    
                    // Check Alt key states (both left and right) - also known as Menu key
                    let lalt_state = GetAsyncKeyState(VK_LMENU) as u16;
                    let ralt_state = GetAsyncKeyState(VK_RMENU) as u16;
                    let alt_down = (lalt_state & 0x8000) != 0 || (ralt_state & 0x8000) != 0;
                    
                    // Detect state changes for Shift
                    if shift_down && !shift_pressed {
                        // Shift was just pressed
                        shift_pressed = true;
                        println!("KeyDown: 'Shift' (modifiers: None)");
                        let event_message = "KeyDown: 'Shift' (modifiers: None)".to_string();
                        self.add_to_key_event_queue(event_message).await?;
                    } else if !shift_down && shift_pressed {
                        // Shift was just released
                        shift_pressed = false;
                        println!("KeyUp: 'Shift' (modifiers: None)");
                        let event_message = "KeyUp: 'Shift' (modifiers: None)".to_string();
                        self.add_to_key_event_queue(event_message).await?;
                    }
                    
                    // Detect state changes for Ctrl
                    if ctrl_down && !ctrl_pressed {
                        // Ctrl was just pressed
                        ctrl_pressed = true;
                        println!("KeyDown: 'Ctrl' (modifiers: None)");
                        let event_message = "KeyDown: 'Ctrl' (modifiers: None)".to_string();
                        self.add_to_key_event_queue(event_message).await?;
                    } else if !ctrl_down && ctrl_pressed {
                        // Ctrl was just released
                        ctrl_pressed = false;
                        println!("KeyUp: 'Ctrl' (modifiers: None)");
                        let event_message = "KeyUp: 'Ctrl' (modifiers: None)".to_string();
                        self.add_to_key_event_queue(event_message).await?;
                    }
                    
                    // Detect state changes for Alt
                    if alt_down && !alt_pressed {
                        // Alt was just pressed
                        alt_pressed = true;
                        println!("KeyDown: 'Alt' (modifiers: None)");
                        let event_message = "KeyDown: 'Alt' (modifiers: None)".to_string();
                        self.add_to_key_event_queue(event_message).await?;
                    } else if !alt_down && alt_pressed {
                        // Alt was just released
                        alt_pressed = false;
                        println!("KeyUp: 'Alt' (modifiers: None)");
                        let event_message = "KeyUp: 'Alt' (modifiers: None)".to_string();
                        self.add_to_key_event_queue(event_message).await?;
                    }
                }

                // Small delay to prevent excessive CPU usage
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        }

        Ok(())
    }

    /// Add a message to the key event queue
    async fn add_to_key_event_queue(&self, message: String) -> Result<()> {
        println!("Adding message to key event queue: {}", message);
        
        // Add the message to the queue
        {
            let mut queue = self.key_event_queue.lock().await;
            queue.push_back(message);
        }
        
        Ok(())
    }

    /// Broadcast data to all connected slaves with appropriate encryption
    async fn broadcast_to_all_slaves(&self, message: String) -> Result<()> {
        println!("Broadcasting message to all connected slaves: {}", message);
        // Convert the message to bytes for transmission
        let data = message.clone().into_bytes(); // Clone to preserve the original for logging

        // Get all connections
        let connections_vec = {
            let connections = self.connections.read().await;
            connections.clone()
        };

        println!("Found {} connected slaves", connections_vec.len());

        // Iterate through all connections and send the data
        for (index, conn_info) in connections_vec.iter().enumerate() {
            println!("Sending message to slave #{}: {}", index, message);
            // Lock the stream temporarily to send data
            let mut stream_lock = conn_info.stream.lock().await;
            
            // Encrypt the data with the specific encryption manager for this connection
            if let Err(e) = send_data_to_stream_with_encryption(&mut *stream_lock, data.clone(), &conn_info.encryption_manager).await {
                eprintln!("Error sending data to slave #{}: {}", index, e);
                // Continue to next connection even if this one fails
            } else {
                println!("Successfully sent message to slave #{}: {}", index, message);
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