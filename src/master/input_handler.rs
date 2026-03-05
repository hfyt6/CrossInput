use crate::{MessageType, KeyboardEvent, MouseEvent, MouseButton, SerializableKey};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;
use rdev::{EventType, Key};

// Import Windows API functions for clearing stdin buffer on Windows systems
#[cfg(target_os = "windows")]
use winapi::um::wincon::FlushConsoleInputBuffer;
#[cfg(target_os = "windows")]
use winapi::um::processenv::GetStdHandle;
#[cfg(target_os = "windows")]
use winapi::um::winbase::STD_INPUT_HANDLE;
#[cfg(target_os = "windows")]
use winapi::um::handleapi::INVALID_HANDLE_VALUE;

/// Input handling functionality for MasterClient
impl super::MasterClient {
    /// Listen for keyboard and mouse input and output events with global interception
    pub async fn listen_for_keyboard_input(&self) -> Result<()> {
        // Use rdev::grab to capture and potentially intercept all keyboard and mouse events globally
        println!("Input listener started.");
        
        // Create a clone of self to use in the callback
        let client = self.clone_for_task();
        let running = Arc::new(std::sync::Mutex::new(true));
        let running_clone = running.clone();
        
        // Start the rdev grabber in a separate thread
        std::thread::spawn(move || {
            // Define the callback function for handling events with potential interception
            if let Err(error) = rdev::grab(move |event| {
                if let EventType::KeyRelease(key) = event.event_type {
                    if matches!(key, Key::ShiftRight) {
                        // Clear the stdin buffer before stopping the listener
                        clear_stdin_buffer();
                        
                        // Stop the listener by setting running to false
                        let flag = *running_clone.lock().unwrap();
                        
                        *running_clone.lock().unwrap() = !flag;
                        if !flag {
                            println!("### output continue! press right-shift to pause! ###");
                        } else {
                            println!("### output pause! press right-shift to continue! ###");
                        }
                        return Some(event);
                    }
                }

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
                        // Some(event)
                        None
                    },
                    EventType::ButtonRelease(button) => {
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
                        // Create the mouse event message
                        let mouse_event = MessageType::Mouse {
                            event: MouseEvent::Scroll {
                                delta_x: delta_x as i32,
                                delta_y: (-delta_y) as i32,
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

        Ok(())
    }
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

/// Clears the standard input buffer on Windows systems
#[cfg(target_os = "windows")]
fn clear_stdin_buffer() {
    unsafe {
        let stdin_handle = GetStdHandle(STD_INPUT_HANDLE);
        if stdin_handle != INVALID_HANDLE_VALUE {
            FlushConsoleInputBuffer(stdin_handle);
        }
    }
}

/// Placeholder for other platforms (stdin clearing is platform-specific)
#[cfg(not(target_os = "windows"))]
fn clear_stdin_buffer() {
    // On non-Windows platforms, we could implement alternative approaches
    // For now, this is a no-op
}
