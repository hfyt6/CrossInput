use crate::{MessageType, KeyboardEvent, MouseEvent, MouseButton, SerializableKey};
use crate::encryption::EncryptionManager;
use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration};
use rdev::{simulate, EventType, Key, Button};
use enigo::{Enigo, Settings, Direction};
use enigo::Mouse;
use lazy_static::lazy_static;

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
                                // Decrypt payload if encrypted - this contains serialized keyboard/mouse events
                                let decrypted_payload = encryption_manager.decrypt(&payload)?;
                                
                                // Try to deserialize the decrypted payload as a MessageType
                                match serde_json::from_slice(&decrypted_payload) as Result<MessageType, _> {
                                    Ok(inner_message) => {
                                        match inner_message {
                                            MessageType::Keyboard { event } => {
                                                println!("Received keyboard event: {:?}", event);
                                                if let Err(e) = simulate_keyboard_event_from_struct(event) {
                                                    eprintln!("Error simulating keyboard event: {}", e);
                                                }
                                            },
                                            MessageType::Mouse { event } => {
                                                println!("Received mouse event: {:?}", event);
                                                if let Err(e) = simulate_mouse_event_from_struct(event) {
                                                    eprintln!("Error simulating mouse event: {}", e);
                                                }
                                            },
                                            _ => {
                                                println!("Received unknown message type in payload: {:?}", inner_message);
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        // If deserialization fails, treat as old string format for backward compatibility
                                        let event_str = String::from_utf8_lossy(&decrypted_payload);
                                        println!("Received data: {:?}", event_str);
                                        
                                        // Parse and simulate keyboard events from string format
                                        if let Err(e) = simulate_keyboard_event(&event_str) {
                                            eprintln!("Error simulating keyboard event: {}", e);
                                        }
                                    }
                                }
                            },
                            MessageType::Keyboard { event } => {
                                println!("Received keyboard event: {:?}", event);
                                if let Err(e) = simulate_keyboard_event_from_struct(event) {
                                    eprintln!("Error simulating keyboard event: {}", e);
                                }
                            },
                            MessageType::Mouse { event } => {
                                println!("Received mouse event: {:?}", event);
                                if let Err(e) = simulate_mouse_event_from_struct(event) {
                                    eprintln!("Error simulating mouse event: {}", e);
                                }
                            },
                            MessageType::Heartbeat => {
                                println!("Received heartbeat");
                            },
                            MessageType::Disconnect => {
                                println!("Received disconnect signal");
                                break;
                            },
                            MessageType::Authenticate { .. } => {
                                // Ignore authenticate messages in this context
                                println!("Received unexpected authenticate message");
                            },
                            MessageType::AuthResponse { .. } => {
                                // Ignore auth response messages in this context
                                println!("Received unexpected auth response message");
                            },
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

/// Simulate keyboard events from structured data
fn simulate_keyboard_event_from_struct(event: KeyboardEvent) -> Result<()> {
    match event {
        KeyboardEvent::Press { key } => {
            let key_clone = key.clone();
            simulate(&EventType::KeyPress(key_clone.into()))
                .map_err(|_| anyhow::anyhow!("Failed to simulate key press"))?;
            println!("Simulated key press: {:?}", key);
        },
        KeyboardEvent::Release { key } => {
            let key_clone = key.clone();
            simulate(&EventType::KeyRelease(key_clone.into()))
                .map_err(|_| anyhow::anyhow!("Failed to simulate key release"))?;
            println!("Simulated key release: {:?}", key);
        },
    }
    Ok(())
}

/// Simulate mouse events from structured data
fn simulate_mouse_event_from_struct(event: MouseEvent) -> Result<()> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| {
        anyhow::anyhow!("Failed to initialize Enigo: {}", e)
    })?;

    match event {
        MouseEvent::ButtonPress { button } => {
            let enigo_button = match button {
                MouseButton::Left => enigo::Button::Left,
                MouseButton::Right => enigo::Button::Right,
                MouseButton::Middle => enigo::Button::Middle,
                MouseButton::Unknown(_) => enigo::Button::Left, // Default to left button
            };
            enigo.button(enigo_button, Direction::Press)
                .map_err(|e| anyhow::anyhow!("Failed to simulate mouse button press: {}", e))?;
            println!("Simulated mouse button press: {:?}", button);
        },
        MouseEvent::ButtonRelease { button } => {
            let enigo_button = match button {
                MouseButton::Left => enigo::Button::Left,
                MouseButton::Right => enigo::Button::Right,
                MouseButton::Middle => enigo::Button::Middle,
                MouseButton::Unknown(_) => enigo::Button::Left, // Default to left button
            };
            enigo.button(enigo_button, Direction::Release)
                .map_err(|e| anyhow::anyhow!("Failed to simulate mouse button release: {}", e))?;
            println!("Simulated mouse button release: {:?}", button);
        },
        MouseEvent::Move { x, y } => {
            enigo.move_mouse(x as i32, y as i32, enigo::Coordinate::Abs)
                .map_err(|e| anyhow::anyhow!("Failed to simulate mouse move: {}", e))?;
            println!("Simulated mouse move to: ({}, {})", x, y);
        },
        MouseEvent::Scroll { delta_x, delta_y } => {
            // Handle horizontal scrolling (delta_x) and vertical scrolling (delta_y)
            if delta_y != 0 {
                enigo.scroll(delta_y as i32, enigo::Axis::Vertical)
                    .map_err(|e| anyhow::anyhow!("Failed to simulate mouse scroll: {}", e))?;
            }
            if delta_x != 0 {
                enigo.scroll(delta_x as i32, enigo::Axis::Horizontal)
                    .map_err(|e| anyhow::anyhow!("Failed to simulate mouse horizontal scroll: {}", e))?;
            }
            println!("Simulated mouse wheel scroll: dx={}, dy={}", delta_x, delta_y);
        },
    }
    Ok(())
}

/// Parse and simulate keyboard events from the received string
fn simulate_keyboard_event(event_str: &str) -> Result<()> {
    // Parse the event string to determine if it's a keydown or keyup event
    if event_str.starts_with("KeyDown:") {
        // Extract the key name from the event string
        // Format: "KeyDown: KEY_NAME (timestamp: ...)"
        if let Some(key_part) = event_str.strip_prefix("KeyDown: ").and_then(|s| s.split(" (timestamp:").next()) {
            // Parse the key from the string representation
            if let Some(key) = parse_key_from_string(key_part.trim()) {
                // Simulate key press event
                simulate(&EventType::KeyPress(key))
                    .map_err(|_| anyhow::anyhow!("Failed to simulate key press"))?;
                println!("Simulated key press: {:?}", key);
            } else {
                eprintln!("Could not parse key from string: {}", key_part);
            }
        }
    } else if event_str.starts_with("KeyUp:") {
        // Extract the key name from the event string
        // Format: "KeyUp: KEY_NAME (timestamp: ...)"
        if let Some(key_part) = event_str.strip_prefix("KeyUp: ").and_then(|s| s.split(" (timestamp:").next()) {
            // Parse the key from the string representation
            if let Some(key) = parse_key_from_string(key_part.trim()) {
                // Simulate key release event
                simulate(&EventType::KeyRelease(key))
                    .map_err(|_| anyhow::anyhow!("Failed to simulate key release"))?;
                println!("Simulated key release: {:?}", key);
            } else {
                eprintln!("Could not parse key from string: {}", key_part);
            }
        }
    } else {
        // Not a keyboard event, ignore
        println!("Ignoring non-keyboard event: {}", event_str);
    }
    
    Ok(())
}

/// Parse a key from its string representation
fn parse_key_from_string(key_str: &str) -> Option<Key> {
    // Remove any quotes around the key name if present
    let clean_key_str = key_str.trim_matches('"');
    
    // Match common keys based on rdev's Key enum variants
    match clean_key_str {
        "Alt" => Some(Key::Alt),
        "AltGr" => Some(Key::AltGr),
        "Backspace" => Some(Key::Backspace),
        "CapsLock" => Some(Key::CapsLock),
        "ControlLeft" => Some(Key::ControlLeft),
        "ControlRight" => Some(Key::ControlRight),
        "Delete" => Some(Key::Delete),
        "DownArrow" => Some(Key::DownArrow),
        "End" => Some(Key::End),
        "Escape" => Some(Key::Escape),
        "F1" => Some(Key::F1),
        "F2" => Some(Key::F2),
        "F3" => Some(Key::F3),
        "F4" => Some(Key::F4),
        "F5" => Some(Key::F5),
        "F6" => Some(Key::F6),
        "F7" => Some(Key::F7),
        "F8" => Some(Key::F8),
        "F9" => Some(Key::F9),
        "F10" => Some(Key::F10),
        "F11" => Some(Key::F11),
        "F12" => Some(Key::F12),
        "Home" => Some(Key::Home),
        "LeftArrow" => Some(Key::LeftArrow),
        "MetaLeft" => Some(Key::MetaLeft),
        "MetaRight" => Some(Key::MetaRight),
        "PageDown" => Some(Key::PageDown),
        "PageUp" => Some(Key::PageUp),
        "Return" => Some(Key::Return),
        "RightArrow" => Some(Key::RightArrow),
        "ShiftLeft" => Some(Key::ShiftLeft),
        "ShiftRight" => Some(Key::ShiftRight),
        "Space" => Some(Key::Space),
        "Tab" => Some(Key::Tab),
        "UpArrow" => Some(Key::UpArrow),
        "PrintScreen" => Some(Key::PrintScreen),
        "ScrollLock" => Some(Key::ScrollLock),
        "Pause" => Some(Key::Pause),
        "Insert" => Some(Key::Insert),
        "NumLock" => Some(Key::NumLock),
        "BackQuote" => Some(Key::BackQuote),
        "Minus" => Some(Key::Minus),
        "Equal" => Some(Key::Equal),
        "LeftBracket" => Some(Key::LeftBracket),
        "RightBracket" => Some(Key::RightBracket),
        "BackSlash" => Some(Key::BackSlash),
        "SemiColon" => Some(Key::SemiColon), // Correct spelling
        "Quote" => Some(Key::Quote),
        "Comma" => Some(Key::Comma),
        "Dot" => Some(Key::Dot),
        "Slash" => Some(Key::Slash),
        "Enter" => Some(Key::Return), // Map Enter to Return
        "KeyA" => Some(Key::KeyA),
        "KeyB" => Some(Key::KeyB),
        "KeyC" => Some(Key::KeyC),
        "KeyD" => Some(Key::KeyD),
        "KeyE" => Some(Key::KeyE),
        "KeyF" => Some(Key::KeyF),
        "KeyG" => Some(Key::KeyG),
        "KeyH" => Some(Key::KeyH),
        "KeyI" => Some(Key::KeyI),
        "KeyJ" => Some(Key::KeyJ),
        "KeyK" => Some(Key::KeyK),
        "KeyL" => Some(Key::KeyL),
        "KeyM" => Some(Key::KeyM),
        "KeyN" => Some(Key::KeyN),
        "KeyO" => Some(Key::KeyO),
        "KeyP" => Some(Key::KeyP),
        "KeyQ" => Some(Key::KeyQ),
        "KeyR" => Some(Key::KeyR),
        "KeyS" => Some(Key::KeyS),
        "KeyT" => Some(Key::KeyT),
        "KeyU" => Some(Key::KeyU),
        "KeyV" => Some(Key::KeyV),
        "KeyW" => Some(Key::KeyW),
        "KeyX" => Some(Key::KeyX),
        "KeyY" => Some(Key::KeyY),
        "KeyZ" => Some(Key::KeyZ),
        "0" => Some(Key::Num0),
        "1" => Some(Key::Num1),
        "2" => Some(Key::Num2),
        "3" => Some(Key::Num3),
        "4" => Some(Key::Num4),
        "5" => Some(Key::Num5),
        "6" => Some(Key::Num6),
        "7" => Some(Key::Num7),
        "8" => Some(Key::Num8),
        "9" => Some(Key::Num9),
        _ => {
            // Try to parse as a single character if it's a quoted character
            if clean_key_str.len() == 1 {
                let ch = clean_key_str.chars().next().unwrap();
                match ch {
                    'a' | 'A' => Some(Key::KeyA),
                    'b' | 'B' => Some(Key::KeyB),
                    'c' | 'C' => Some(Key::KeyC),
                    'd' | 'D' => Some(Key::KeyD),
                    'e' | 'E' => Some(Key::KeyE),
                    'f' | 'F' => Some(Key::KeyF),
                    'g' | 'G' => Some(Key::KeyG),
                    'h' | 'H' => Some(Key::KeyH),
                    'i' | 'I' => Some(Key::KeyI),
                    'j' | 'J' => Some(Key::KeyJ),
                    'k' | 'K' => Some(Key::KeyK),
                    'l' | 'L' => Some(Key::KeyL),
                    'm' | 'M' => Some(Key::KeyM),
                    'n' | 'N' => Some(Key::KeyN),
                    'o' | 'O' => Some(Key::KeyO),
                    'p' | 'P' => Some(Key::KeyP),
                    'q' | 'Q' => Some(Key::KeyQ),
                    'r' | 'R' => Some(Key::KeyR),
                    's' | 'S' => Some(Key::KeyS),
                    't' | 'T' => Some(Key::KeyT),
                    'u' | 'U' => Some(Key::KeyU),
                    'v' | 'V' => Some(Key::KeyV),
                    'w' | 'W' => Some(Key::KeyW),
                    'x' | 'X' => Some(Key::KeyX),
                    'y' | 'Y' => Some(Key::KeyY),
                    'z' | 'Z' => Some(Key::KeyZ),
                    '0' => Some(Key::Num0),
                    '1' => Some(Key::Num1),
                    '2' => Some(Key::Num2),
                    '3' => Some(Key::Num3),
                    '4' => Some(Key::Num4),
                    '5' => Some(Key::Num5),
                    '6' => Some(Key::Num6),
                    '7' => Some(Key::Num7),
                    '8' => Some(Key::Num8),
                    '9' => Some(Key::Num9),
                    ' ' => Some(Key::Space),
                    '\n' | '\r' => Some(Key::Return),
                    '\t' => Some(Key::Tab),
                    _ => None,
                }
            } else {
                // For unknown keys, return None
                None
            }
        }
    }
}
