use std::io::{self, ErrorKind};
use std::path::Path; // Needed for filesystem paths if used
use std::time::Duration;
use serde::{Deserialize, Serialize};
// Fix imports for interprocess
use interprocess::local_socket::{
    tokio::{prelude::*, Stream}, // Use Stream directly and prelude for traits
    GenericNamespaced, GenericFilePath, ToFsName, ToNsName, Name,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};
// MPSC channels for task communication
use tokio::sync::mpsc;

// --- Shared Message Structures ---
// These structs define the communication protocol.
// Ideally, move these to a shared crate later (e.g., `shared_types`).
#[derive(Serialize, Deserialize, Debug, Clone)]
struct Message {
    action: String,
    task_id: String,
    task: Task,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Task {
    steps: Vec<Step>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
enum Step {
    #[serde(rename = "navigate")]
    Navigate { url: String },
    #[serde(rename = "scrape")]
    Scrape { config: serde_json::Value }, // Keep config generic for broker
    #[serde(rename = "click")]
    Click {
        selector: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        wait_for_nav: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout: Option<u32>,
    },
    #[serde(rename = "fill")]
    Fill {
        selector: String,
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        dispatch_events: Option<Vec<String>>,
    },
    #[serde(rename = "wait_for_selector")]
    WaitForSelector {
        selector: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        state: Option<String>,
        timeout: u32,
    },
    #[serde(rename = "wait_for_timeout")]
    WaitForTimeout { timeout: u32 },
    #[serde(rename = "extract")]
    Extract {
        selector: String,
        target: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        attribute_name: Option<String>,
        variable_name: String,
    },
    // Add other step types as needed, ensuring they match the Main App's expectations
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct ExtensionResponse {
    action: String, // e.g., "task_result"
    task_id: String,
    success: bool,
    // Use serde_json::Value for flexibility, or define specific result structs
    result: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// TaskResult and StepResult might not be needed directly in the broker
// if it just forwards opaque JSON values. Keep them if you parse results here.
// #[derive(Deserialize, Serialize, Debug, Clone)]
// struct TaskResult { ... }
// #[derive(Deserialize, Serialize, Debug, Clone)]
// struct StepResult { ... }

// --- End of Shared Message Structures ---

// Constants
const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024; // 10MB limit for messages

// Define a unique name for the IPC endpoint using interprocess helpers
// This function now returns the Name type directly.
fn get_ipc_endpoint_name() -> io::Result<Name<'static> > {
    // Choose a unique name. Using a namespaced name is generally preferred
    // for cross-platform compatibility when supported.
    let name = "com.yourcompany.projectagentis.broker.sock";

    // Try creating a namespaced name first
    if GenericNamespaced::is_supported() {
        name.to_ns_name::<GenericNamespaced>()
            .map_err(|e| io::Error::new(ErrorKind::Other, e))
    } else {
        // Fallback to a filesystem path if namespaced is not supported
        // IMPORTANT: Ensure the directory exists and has correct permissions.
        // Using /tmp/ might be problematic on some systems or in sandboxed environments.
        // Consider a more robust location like user data directories.
        let path_str = format!("/tmp/{}", name);
        // Create a static string to avoid reference issues
        String::from(path_str).to_fs_name::<GenericFilePath>()
            .map_err(|e| io::Error::new(ErrorKind::Other, e))
    }
}


#[tokio::main]
async fn main() -> io::Result<()> {
    // Initialize logger (e.g., RUST_LOG=info cargo run --package rzn_broker)
    env_logger::init();
    log::info!("Broker starting...");

    // 1. Get the IPC endpoint name
    let ipc_endpoint = get_ipc_endpoint_name()?; // Use the updated function

    log::info!("Attempting to connect to Main App via IPC: {:?}", ipc_endpoint);

    // TODO: Add logic here to *launch* the Main App if connection fails initially.
    // For now, we just retry and exit if it ultimately fails.
    let ipc_stream = match connect_to_main_app(&ipc_endpoint).await {
        Ok(stream) => {
            log::info!("Successfully connected to Main App via IPC.");
            stream
        }
        Err(e) => {
            log::error!("Failed to connect to Main App after retries: {}", e);
            // In a real scenario, you might try launching the main app here.
            // For now, we exit if the main app isn't running/listening.
            log::error!("Broker exiting because Main App connection failed.");
            return Err(e); // Exit broker if connection fails
        }
    };
    // Split the IPC stream into owned read/write halves
    let (ipc_reader, ipc_writer) = tokio::io::split(ipc_stream);

    // 2. Setup Native Messaging (stdin/stdout)
    let native_stdin = tokio::io::stdin();
    let native_stdout = tokio::io::stdout();
    // Use BufReader/BufWriter for potentially better performance
    let native_reader = BufReader::new(native_stdin);
    let native_writer = BufWriter::new(native_stdout);

    // 3. Create channels for communication between tasks
    // Channel for messages from Extension (NativeRead) to Main App (IpcWrite)
    let (ext_to_ipc_tx, ext_to_ipc_rx) = mpsc::channel::<Vec<u8>>(10);
    // Channel for messages from Main App (IpcRead) to Extension (NativeWrite)
    let (ipc_to_ext_tx, ipc_to_ext_rx) = mpsc::channel::<Vec<u8>>(10);

    // 4. Spawn Tasks for Relaying Messages

    // Task: Read from Extension (stdin) -> Send to IPC Channel (ext_to_ipc_tx)
    let ext_reader_task = tokio::spawn(handle_native_read(native_reader, ext_to_ipc_tx));

    // Task: Read from IPC Channel (ext_to_ipc_rx) -> Write to Main App (IPC writer)
    let ipc_writer_task = tokio::spawn(handle_ipc_write(ipc_writer, ext_to_ipc_rx));

    // Task: Read from Main App (IPC reader) -> Send to Extension Channel (ipc_to_ext_tx)
    let ipc_reader_task = tokio::spawn(handle_ipc_read(ipc_reader, ipc_to_ext_tx));

    // Task: Read from Extension Channel (ipc_to_ext_rx) -> Write to Extension (stdout)
    let ext_writer_task = tokio::spawn(handle_native_write(native_writer, ipc_to_ext_rx));


    // 5. Wait for any task to finish (indicates disconnection or error)
    // If any task exits, the broker should probably shut down.
    tokio::select! {
        res = ext_reader_task => log::info!("Extension reader task finished: {:?}", res),
        res = ipc_writer_task => log::info!("IPC writer task finished: {:?}", res),
        res = ipc_reader_task => log::info!("IPC reader task finished: {:?}", res),
        res = ext_writer_task => log::info!("Extension writer task finished: {:?}", res),
    }

    log::info!("Broker shutting down.");
    Ok(())
}

// --- Task Implementations ---

/// Reads messages from the browser extension (stdin) and sends them to the IPC channel.
async fn handle_native_read(
    mut reader: BufReader<tokio::io::Stdin>,
    tx: mpsc::Sender<Vec<u8>>
) {
    log::info!("NativeRead: Waiting for messages from extension...");
    loop {
        match read_message_bytes(&mut reader, "NativeRead").await {
            Ok(Some(message_bytes)) => {
                // Basic validation/logging: Try to parse minimally
                if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&message_bytes) {
                    log::info!("NativeRead: Received message (action: {}, task_id: {})",
                             value.get("action").and_then(|v| v.as_str()).unwrap_or("N/A"),
                             value.get("task_id").and_then(|v| v.as_str()).unwrap_or("N/A"));
                } else {
                    log::warn!("NativeRead: Received message, but failed to parse as JSON for logging.");
                }

                // Send the raw bytes to the channel for the IPC writer task
                if tx.send(message_bytes).await.is_err() {
                    log::error!("NativeRead: IPC channel closed. Stopping reading from extension.");
                    break; // Exit task if channel is closed
                }
            }
            Ok(None) => {
                log::info!("NativeRead: Extension disconnected (stdin closed).");
                break; // Exit task on clean disconnect
            }
            Err(e) => {
                log::error!("NativeRead: Error reading from extension: {}", e);
                break; // Exit task on error
            }
        }
    }
    log::info!("NativeRead: Task finished.");
    // tx is dropped here, signaling the receiver
}

/// Reads messages from the IPC channel and writes them to the Main Application (IPC socket).
async fn handle_ipc_write(
    mut writer: impl AsyncWrite + Unpin, // Generic over AsyncWrite + Unpin
    mut rx: mpsc::Receiver<Vec<u8>>
) {
    log::info!("IpcWrite: Waiting for messages to send to Main App...");
    // Process messages from the channel until it's closed
    while let Some(message_bytes) = rx.recv().await {
         // Basic validation/logging
         if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&message_bytes) {
            log::info!("IpcWrite: Forwarding message to Main App (action: {}, task_id: {})",
                     value.get("action").and_then(|v| v.as_str()).unwrap_or("N/A"),
                     value.get("task_id").and_then(|v| v.as_str()).unwrap_or("N/A"));
        } else {
            log::warn!("IpcWrite: Forwarding message, but failed to parse as JSON for logging.");
        }

        // Write the raw bytes to the IPC stream
        if let Err(e) = write_message_bytes(&mut writer, &message_bytes, "IpcWrite").await {
            log::error!("IpcWrite: Error writing to Main App: {}", e);
            break; // Exit task on write error
        }
    }
     // rx.recv() returned None, meaning the sender (NativeRead) has finished/dropped.
     log::info!("IpcWrite: Channel closed. Task finished.");
}

/// Reads messages from the Main Application (IPC socket) and sends them to the Native channel.
async fn handle_ipc_read(
    mut reader: impl AsyncRead + Unpin, // Generic over AsyncRead + Unpin
    tx: mpsc::Sender<Vec<u8>>
) {
    log::info!("IpcRead: Waiting for messages from Main App...");
    loop {
        match read_message_bytes(&mut reader, "IpcRead").await {
            Ok(Some(message_bytes)) => {
                 // Basic validation/logging
                 if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&message_bytes) {
                    log::info!("IpcRead: Received message from Main App (action: {}, task_id: {})",
                             value.get("action").and_then(|v| v.as_str()).unwrap_or("N/A"),
                             value.get("task_id").and_then(|v| v.as_str()).unwrap_or("N/A"));
                } else {
                    log::warn!("IpcRead: Received message, but failed to parse as JSON for logging.");
                }

                // Send the raw bytes to the channel for the Native writer task
                if tx.send(message_bytes).await.is_err() {
                    log::error!("IpcRead: Native channel closed. Stopping reading from Main App.");
                    break; // Exit task if channel is closed
                }
            }
            Ok(None) => {
                log::info!("IpcRead: Main App disconnected (IPC closed).");
                break; // Exit task on clean disconnect
            }
            Err(e) => {
                log::error!("IpcRead: Error reading from Main App: {}", e);
                break; // Exit task on error
            }
        }
    }
     log::info!("IpcRead: Task finished.");
     // tx is dropped here, signaling the receiver
}

/// Reads messages from the Native channel and writes them to the browser extension (stdout).
async fn handle_native_write(
    mut writer: BufWriter<tokio::io::Stdout>,
    mut rx: mpsc::Receiver<Vec<u8>>
) {
    log::info!("NativeWrite: Waiting for messages to send to extension...");
    // Process messages from the channel until it's closed
    while let Some(message_bytes) = rx.recv().await {
         // Basic validation/logging
         if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&message_bytes) {
            log::info!("NativeWrite: Forwarding message to extension (action: {}, task_id: {})",
                     value.get("action").and_then(|v| v.as_str()).unwrap_or("N/A"),
                     value.get("task_id").and_then(|v| v.as_str()).unwrap_or("N/A"));
        } else {
            log::warn!("NativeWrite: Forwarding message, but failed to parse as JSON for logging.");
        }

        // Write the raw bytes to stdout for the extension
        if let Err(e) = write_message_bytes(&mut writer, &message_bytes, "NativeWrite").await {
            log::error!("NativeWrite: Error writing to extension: {}", e);
            break; // Exit task on write error
        }
    }
    // rx.recv() returned None, meaning the sender (IpcRead) has finished/dropped.
    log::info!("NativeWrite: Channel closed. Task finished.");
}


// --- Helper Functions ---

/// Attempts to connect to the Main Application's IPC endpoint using Stream::connect with retries.
async fn connect_to_main_app(
    endpoint: &Name<'_>,
) -> io::Result<Stream> {
    let mut attempts = 0;
    let max_attempts = 5;
    let retry_delay = Duration::from_secs(1);

    loop {
        match Stream::connect(endpoint.clone()).await {
            Ok(stream) => return Ok(stream),
            Err(e) => {
                attempts += 1;
                log::warn!(
                    "IPC connection attempt {}/{} failed: {}. Retrying in {:?}...",
                    attempts,
                    max_attempts,
                    e,
                    retry_delay
                );
                if attempts >= max_attempts {
                    log::error!("Max IPC connection attempts reached.");
                    return Err(e);
                }
                tokio::time::sleep(retry_delay).await;
            }
        }
    }
}

/// Reads a message prefixed with a 4-byte little-endian length.
/// Generic over any AsyncRead + Unpin source.
async fn read_message_bytes<R: AsyncRead + Unpin>(
    reader: &mut R,
    log_prefix: &str, // For clearer logging
) -> io::Result<Option<Vec<u8>>> {
    let mut len_bytes = [0u8; 4];
    // Read the length prefix
    match reader.read_exact(&mut len_bytes).await {
        Ok(_) => {}
        // If EOF is encountered while reading length, it's a clean disconnect.
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
            log::debug!("{}: Connection closed cleanly while reading length.", log_prefix);
            return Ok(None);
        }
        Err(e) => {
            log::error!("{}: Error reading message length: {}", log_prefix, e);
            return Err(e);
        }
    }

    let len = u32::from_le_bytes(len_bytes) as usize;
    // log::trace!("{}: Message length: {}", log_prefix, len); // Use trace for noisy logs

    // Protect against excessively large messages
    if len > MAX_MESSAGE_SIZE {
        let err_msg = format!("Message length {} exceeds limit {}", len, MAX_MESSAGE_SIZE);
        log::error!("{}: {}", log_prefix, err_msg);
        return Err(io::Error::new(ErrorKind::InvalidData, err_msg));
    }
    // Handle zero-length messages if necessary (might indicate keep-alive or error)
    if len == 0 {
        log::warn!("{}: Received message length 0.", log_prefix);
        // Decide how to handle: return empty vec, or treat as error?
        return Ok(Some(Vec::new())); // Return empty vec for now
    }

    // Allocate buffer and read the message body
    let mut buffer = vec![0u8; len];
    match reader.read_exact(&mut buffer).await {
        Ok(_) => {
            // log::trace!("{}: Successfully read message body ({} bytes)", log_prefix, len);
            Ok(Some(buffer))
        },
        // If EOF is encountered *during* body read, it's an unexpected closure.
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
            log::error!("{}: Connection closed unexpectedly while reading message body (expected {} bytes).", log_prefix, len);
            Err(e) // Return error because message is incomplete
        }
        Err(e) => {
            log::error!("{}: Error reading message body: {}", log_prefix, e);
            Err(e)
        }
    }
}

/// Writes a message prefixed with a 4-byte little-endian length.
/// Generic over any AsyncWrite + Unpin sink.
async fn write_message_bytes<W: AsyncWrite + Unpin>(
    writer: &mut W,
    message_bytes: &[u8],
    log_prefix: &str, // For clearer logging
) -> io::Result<()> {
    let len = message_bytes.len();
    // Protect against sending excessively large messages
    if len > MAX_MESSAGE_SIZE {
         let err_msg = format!("Attempted to send message larger than limit: {} bytes", len);
         log::error!("{}: {}", log_prefix, err_msg);
        return Err(io::Error::new(ErrorKind::InvalidInput, err_msg));
    }

    // log::trace!("{}: Sending message ({} bytes)", log_prefix, len);
    // Write length prefix
    writer.write_all(&(len as u32).to_le_bytes()).await?;
    // Write message body
    writer.write_all(message_bytes).await?;
    // Flush the writer to ensure data is sent
    writer.flush().await?;
    // log::trace!("{}: Message flushed.", log_prefix);
    Ok(())
}

// Remove old CLI-specific functions like create_structured_task_message, handle_extension_response, etc.
// The broker's job is just to relay bytes. Parsing/handling responses happens in the Main App.
