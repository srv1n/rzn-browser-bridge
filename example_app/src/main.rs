use std::io::{self, ErrorKind};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
// Use interprocess's Tokio integration for local sockets
use interprocess::local_socket::{
    tokio::{prelude::*, Listener, Stream}, // Use Listener and Stream
    GenericNamespaced, GenericFilePath, ToFsName, ToNsName, Name, ListenerOptions, // Import necessary types/traits
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc; // Although not used for sending between tasks here, keep for consistency if needed later

// --- Shared Message Structures (Copied from Broker for now) ---
// IMPORTANT: In a real project, move these to a shared crate (e.g., `shared_types`)
// to avoid duplication and ensure consistency.

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Message {
    action: String,
    task_id: String,
    // Make task optional or use a different struct for simple pings
    task: Option<Task>, // Example: Make task optional for ping
    // Add other fields as needed for different message types
    data: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Task {
    steps: Vec<Step>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
enum Step {
    // Define steps if needed, or keep empty if only handling pings initially
    #[serde(rename = "navigate")] Navigate { url: String },
    // ... other steps
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct ExtensionResponse {
    action: String, // e.g., "pong", "task_result"
    task_id: String, // Echo task_id if available, else use placeholder
    success: bool,
    result: Option<serde_json::Value>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}
// --- End of Shared Message Structures ---

// Constants
const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024; // 10MB limit

// --- IPC Endpoint Name (MUST match the Broker's) ---
fn get_ipc_endpoint_name() -> io::Result<Name<'static> > {
    let name = "com.yourcompany.projectagentis.broker.sock";
    if GenericNamespaced::is_supported() {
        name.to_ns_name::<GenericNamespaced>()
            .map_err(|e| io::Error::new(ErrorKind::Other, e))
    } else {
        let path_str = format!("/tmp/{}", name);
        // Ensure the path exists or handle creation if needed
        // For simplicity, we assume /tmp exists. Use directories crate for robust paths.
        String::from(path_str).to_fs_name::<GenericFilePath>()
            .map_err(|e| io::Error::new(ErrorKind::Other, e))
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    env_logger::init();
    log::info!("Example App Server starting...");

    // 1. Get the IPC endpoint name
    let ipc_endpoint = get_ipc_endpoint_name()?;
    log::info!("Attempting to listen on IPC endpoint: {:?}", ipc_endpoint);

    // 2. Set up the listener options
    let opts = ListenerOptions::new().name(ipc_endpoint.clone());

    // 3. Create the listener
    let listener = match opts.create_tokio() {
        Ok(listener) => {
            log::info!("Server listening on {:?}", ipc_endpoint);
            listener
        }
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
            // Handle case where the socket file/pipe exists (e.g., from a previous crash)
            log::error!(
                "IPC endpoint {:?} already in use. Attempting to clean up...",
                ipc_endpoint
            );
            // On Unix, try removing the socket file. This is potentially racy.
            #[cfg(unix)]
            {
                // For filesystem-based sockets on Unix, try to remove the file
                // Create a path using the same logic as in get_ipc_endpoint_name
                let socket_name = "com.yourcompany.projectagentis.broker.sock";
                let path_str = format!("/tmp/{}", socket_name);
                let path = std::path::Path::new(&path_str);
                
                if path.exists() {
                    match std::fs::remove_file(path) {
                        Ok(_) => {
                            log::info!("Removed stale socket file: {:?}", path);
                            // Try creating the listener again with new options
                            let new_opts = ListenerOptions::new().name(ipc_endpoint.clone());
                            new_opts.create_tokio()?
                        }
                        Err(remove_err) => {
                            log::error!("Failed to remove stale socket file {:?}: {}", path, remove_err);
                            return Err(e);
                        }
                    }
                } else {
                    log::error!("Socket file expected but not found at: {:?}", path);
                    return Err(e);
                }
            }
            // On Windows, named pipes usually clean up better, but retrying might still be needed.
            #[cfg(not(unix))]
            {
                 log::error!("IPC endpoint {:?} already in use. Please ensure no other instance is running.", ipc_endpoint);
                 return Err(e);
            }
        }
        Err(e) => {
            log::error!("Failed to create IPC listener: {}", e);
            return Err(e);
        }
    };

    // 4. Accept connections in a loop
    loop {
        match listener.accept().await {
            Ok(stream) => {
                log::info!("Broker connected!");
                // Spawn a task to handle this connection
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream).await {
                        log::error!("Error handling connection: {}", e);
                    }
                    log::info!("Broker disconnected.");
                });
            }
            Err(e) => {
                log::error!("Failed to accept connection: {}", e);
                // Decide if the error is recoverable or if we should break the loop
                // For now, just log and continue trying to accept
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

/// Handles a single connection from the broker
async fn handle_connection(stream: Stream) -> io::Result<()> {
    // Split the stream for reading and writing
    // Use tokio::io::split as the broker does, for consistency
    let (mut reader, mut writer) = tokio::io::split(stream);

    loop {
        // Read message from broker
        match read_message_bytes(&mut reader, "ExampleAppRead").await {
            Ok(Some(message_bytes)) => {
                if message_bytes.is_empty() {
                    log::warn!("Received empty message from broker.");
                    continue;
                }

                // Attempt to deserialize the message (e.g., into the generic Message struct)
                match serde_json::from_slice::<Message>(&message_bytes) {
                    Ok(received_msg) => {
                        log::info!("Received message: {:?}", received_msg);

                        // --- Simple Echo/Pong Logic ---
                        let response_action = match received_msg.action.as_str() {
                            "ping" => "pong".to_string(),
                            "perform_task" => "task_result".to_string(), // Acknowledge task receipt
                            _ => "unknown_action_response".to_string(),
                        };

                        // Create a simple response
                        let response = ExtensionResponse {
                            action: response_action,
                            task_id: received_msg.task_id.clone(), // Echo task_id
                            success: true, // Assume success for this simple test
                            result: Some(serde_json::json!({ "echo": received_msg })), // Echo back received data
                            error: None,
                        };

                        // Serialize the response
                        match serde_json::to_vec(&response) {
                            Ok(response_bytes) => {
                                // Send response back to broker
                                if let Err(e) = write_message_bytes(&mut writer, &response_bytes, "ExampleAppWrite").await {
                                    log::error!("Failed to send response to broker: {}", e);
                                    break; // Stop handling this connection on write error
                                }
                                log::info!("Sent response: {:?}", response);
                            }
                            Err(e) => {
                                log::error!("Failed to serialize response: {}", e);
                                // Decide if we should send an error back or just log
                            }
                        }
                        // --- End Simple Echo/Pong Logic ---

                    }
                    Err(e) => {
                        log::error!("Failed to deserialize message: {}. Raw bytes: {:?}", e, message_bytes);
                        // Optionally send an error response back
                    }
                }
            }
            Ok(None) => {
                // Broker disconnected cleanly
                log::info!("Broker closed connection while reading.");
                break;
            }
            Err(e) => {
                // Error reading from broker
                log::error!("Error reading from broker: {}", e);
                break;
            }
        }
    }
    Ok(())
}


// --- Helper Functions (Copied from Broker) ---
// IMPORTANT: Move these to a shared crate.

/// Reads a message prefixed with a 4-byte little-endian length.
async fn read_message_bytes<R: AsyncRead + Unpin>(
    reader: &mut R,
    log_prefix: &str,
) -> io::Result<Option<Vec<u8>>> {
    let mut len_bytes = [0u8; 4];
    match reader.read_exact(&mut len_bytes).await {
        Ok(_) => {}
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
    if len > MAX_MESSAGE_SIZE {
        let err_msg = format!("Message length {} exceeds limit {}", len, MAX_MESSAGE_SIZE);
        log::error!("{}: {}", log_prefix, err_msg);
        return Err(io::Error::new(ErrorKind::InvalidData, err_msg));
    }
    if len == 0 {
        log::warn!("{}: Received message length 0.", log_prefix);
        return Ok(Some(Vec::new()));
    }

    let mut buffer = vec![0u8; len];
    match reader.read_exact(&mut buffer).await {
        Ok(_) => Ok(Some(buffer)),
        Err(e) if e.kind() == ErrorKind::UnexpectedEof => {
            log::error!("{}: Connection closed unexpectedly while reading message body (expected {} bytes).", log_prefix, len);
            Err(e)
        }
        Err(e) => {
            log::error!("{}: Error reading message body: {}", log_prefix, e);
            Err(e)
        }
    }
}

/// Writes a message prefixed with a 4-byte little-endian length.
async fn write_message_bytes<W: AsyncWrite + Unpin>(
    writer: &mut W,
    message_bytes: &[u8],
    log_prefix: &str,
) -> io::Result<()> {
    let len = message_bytes.len();
    if len > MAX_MESSAGE_SIZE {
         let err_msg = format!("Attempted to send message larger than limit: {} bytes", len);
         log::error!("{}: {}", log_prefix, err_msg);
        return Err(io::Error::new(ErrorKind::InvalidInput, err_msg));
    }

    writer.write_all(&(len as u32).to_le_bytes()).await?;
    writer.write_all(message_bytes).await?;
    writer.flush().await?;
    Ok(())
}
