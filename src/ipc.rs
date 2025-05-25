use crate::config::IpcConfig;
use crate::window_manager::Command;
use crate::{Result, WindowId};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcMessage {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcResponse {
    pub success: bool,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

pub struct IpcServer {
    config: IpcConfig,
    command_sender: mpsc::Sender<Command>,
}

impl IpcServer {
    pub async fn new(config: &IpcConfig, command_sender: mpsc::Sender<Command>) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            command_sender,
        })
    }

    pub async fn start(&self) -> Result<()> {
        let socket_path = &self.config.socket_path;

        // Remove existing socket file if it exists
        if Path::new(socket_path).exists() {
            std::fs::remove_file(socket_path)?;
        }

        let listener = UnixListener::bind(socket_path)?;
        info!("IPC server listening on {}", socket_path);

        let command_sender = self.command_sender.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        debug!("IPC client connected: {:?}", addr);
                        let sender = command_sender.clone();
                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_client(stream, sender).await {
                                error!("Error handling IPC client: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Error accepting IPC connection: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    async fn handle_client(
        stream: UnixStream,
        command_sender: mpsc::Sender<Command>,
    ) -> Result<()> {
        let (reader, writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut writer = writer;
        let mut line = String::new();

        // Set a timeout for client operations
        let client_timeout = Duration::from_secs(30);

        while let Ok(Ok(bytes_read)) = timeout(client_timeout, reader.read_line(&mut line)).await {
            if bytes_read == 0 {
                debug!("IPC client disconnected");
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                line.clear();
                continue;
            }

            debug!("Received IPC message: {}", trimmed);

            let response = match serde_json::from_str::<IpcMessage>(trimmed) {
                Ok(message) => {
                    Self::process_message(message, &command_sender).await
                }
                Err(e) => IpcResponse {
                    success: false,
                    message: format!("Invalid JSON: {}", e),
                    data: None,
                },
            };

            // Send response back to client
            let response_json = match serde_json::to_string(&response) {
                Ok(json) => json,
                Err(e) => {
                    error!("Failed to serialize response: {}", e);
                    serde_json::to_string(&IpcResponse {
                        success: false,
                        message: "Internal server error".to_string(),
                        data: None,
                    }).unwrap_or_else(|_| "{}".to_string())
                }
            };

            if let Err(e) = writer.write_all(response_json.as_bytes()).await {
                error!("Failed to write response: {}", e);
                break;
            }
            if let Err(e) = writer.write_all(b"\n").await {
                error!("Failed to write newline: {}", e);
                break;
            }
            if let Err(e) = writer.flush().await {
                error!("Failed to flush response: {}", e);
                break;
            }

            debug!("Sent response: {}", response_json);
            line.clear();
        }

        debug!("IPC client handler finished");
        Ok(())
    }

    async fn process_message(
        message: IpcMessage,
        command_sender: &mpsc::Sender<Command>,
    ) -> IpcResponse {
        debug!("Processing command: {} with args: {:?}", message.command, message.args);
        
        let command = match message.command.as_str() {
            "focus" => {
                if let Some(id_str) = message.args.get(0) {
                    match id_str.parse::<u32>() {
                        Ok(id) => Command::FocusWindow(WindowId(id)),
                        Err(_) => {
                            return IpcResponse {
                                success: false,
                                message: "Invalid window ID".to_string(),
                                data: None,
                            };
                        }
                    }
                } else {
                    return IpcResponse {
                        success: false,
                        message: "focus command requires window ID argument".to_string(),
                        data: None,
                    };
                }
            }
            "close" => {
                if let Some(id_str) = message.args.get(0) {
                    match id_str.parse::<u32>() {
                        Ok(id) => Command::CloseWindow(WindowId(id)),
                        Err(_) => {
                            return IpcResponse {
                                success: false,
                                message: "Invalid window ID".to_string(),
                                data: None,
                            };
                        }
                    }
                } else {
                    return IpcResponse {
                        success: false,
                        message: "close command requires window ID argument".to_string(),
                        data: None,
                    };
                }
            }
            "move" => {
                if message.args.len() >= 5 {
                    match (
                        message.args[0].parse::<u32>(),
                        message.args[1].parse::<f64>(),
                        message.args[2].parse::<f64>(),
                        message.args[3].parse::<f64>(),
                        message.args[4].parse::<f64>(),
                    ) {
                        (Ok(id), Ok(x), Ok(y), Ok(width), Ok(height)) => {
                            let rect = crate::Rect::new(x, y, width, height);
                            Command::MoveWindow(WindowId(id), rect)
                        }
                        _ => {
                            return IpcResponse {
                                success: false,
                                message: "move command requires: window_id x y width height".to_string(),
                                data: None,
                            };
                        }
                    }
                } else {
                    return IpcResponse {
                        success: false,
                        message: "move command requires: window_id x y width height".to_string(),
                        data: None,
                    };
                }
            }
            "toggle-layout" => Command::ToggleLayout,
            "reload" => Command::ReloadConfig,
            "list" => Command::ListWindows,
            "status" => Command::GetStatus,
            "quit" | "stop" => Command::Quit,
            "ping" => {
                return IpcResponse {
                    success: true,
                    message: "pong".to_string(),
                    data: Some(serde_json::json!({
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                        "version": env!("CARGO_PKG_VERSION")
                    })),
                };
            }
            "help" => {
                return IpcResponse {
                    success: true,
                    message: "Available commands".to_string(),
                    data: Some(serde_json::json!({
                        "commands": [
                            {"name": "focus", "args": ["window_id"], "description": "Focus a window"},
                            {"name": "close", "args": ["window_id"], "description": "Close a window"},
                            {"name": "move", "args": ["window_id", "x", "y", "width", "height"], "description": "Move and resize a window"},
                            {"name": "toggle-layout", "args": [], "description": "Toggle between layout modes"},
                            {"name": "reload", "args": [], "description": "Reload configuration"},
                            {"name": "list", "args": [], "description": "List all windows"},
                            {"name": "status", "args": [], "description": "Get window manager status"},
                            {"name": "ping", "args": [], "description": "Test connection"},
                            {"name": "quit", "args": [], "description": "Stop the window manager"},
                            {"name": "help", "args": [], "description": "Show this help"}
                        ]
                    })),
                };
            }
            _ => {
                return IpcResponse {
                    success: false,
                    message: format!("Unknown command: '{}'. Use 'help' to see available commands.", message.command),
                    data: None,
                };
            }
        };

        // Send command to window manager
        match command_sender.send(command).await {
            Ok(()) => IpcResponse {
                success: true,
                message: "Command sent successfully".to_string(),
                data: None,
            },
            Err(e) => IpcResponse {
                success: false,
                message: format!("Failed to send command: {}", e),
                data: None,
            },
        }
    }
}

pub struct IpcClient {
    socket_path: String,
}

impl IpcClient {
    pub fn new(socket_path: String) -> Self {
        Self { socket_path }
    }

    pub async fn send_command(&self, command: &str, args: Vec<String>) -> Result<IpcResponse> {
        let stream = UnixStream::connect(&self.socket_path).await?;
        let (reader, mut writer) = stream.into_split();

        let message = IpcMessage {
            command: command.to_string(),
            args,
        };

        let message_json = serde_json::to_string(&message)?;
        
        // Send message
        writer.write_all(message_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        // Read response with timeout
        let mut reader = BufReader::new(reader);
        let mut response_line = String::new();
        
        match timeout(Duration::from_secs(10), reader.read_line(&mut response_line)).await {
            Ok(Ok(_)) => {
                let response: IpcResponse = serde_json::from_str(&response_line)?;
                Ok(response)
            }
            Ok(Err(e)) => Err(anyhow::anyhow!("Failed to read response: {}", e)),
            Err(_) => Err(anyhow::anyhow!("Response timeout")),
        }
    }

    pub async fn ping(&self) -> Result<IpcResponse> {
        self.send_command("ping", vec![]).await
    }

    pub async fn focus_window(&self, window_id: WindowId) -> Result<IpcResponse> {
        self.send_command("focus", vec![window_id.0.to_string()]).await
    }

    pub async fn close_window(&self, window_id: WindowId) -> Result<IpcResponse> {
        self.send_command("close", vec![window_id.0.to_string()]).await
    }

    pub async fn move_window(&self, window_id: WindowId, rect: crate::Rect) -> Result<IpcResponse> {
        self.send_command(
            "move",
            vec![
                window_id.0.to_string(),
                rect.x.to_string(),
                rect.y.to_string(),
                rect.width.to_string(),
                rect.height.to_string(),
            ],
        ).await
    }

    pub async fn toggle_layout(&self) -> Result<IpcResponse> {
        self.send_command("toggle-layout", vec![]).await
    }

    pub async fn reload_config(&self) -> Result<IpcResponse> {
        self.send_command("reload", vec![]).await
    }

    pub async fn list_windows(&self) -> Result<IpcResponse> {
        self.send_command("list", vec![]).await
    }

    pub async fn get_status(&self) -> Result<IpcResponse> {
        self.send_command("status", vec![]).await
    }

    pub async fn quit(&self) -> Result<IpcResponse> {
        self.send_command("quit", vec![]).await
    }

    pub async fn help(&self) -> Result<IpcResponse> {
        self.send_command("help", vec![]).await
    }
}

// Utility functions for building a CLI client
impl IpcClient {
    pub async fn run_command(socket_path: &str, command: &str, args: Vec<String>) -> Result<()> {
        let client = IpcClient::new(socket_path.to_string());
        
        let response = client.send_command(command, args).await?;
        
        if response.success {
            println!("✓ {}", response.message);
            if let Some(data) = response.data {
                println!("{}", serde_json::to_string_pretty(&data)?);
            }
        } else {
            eprintln!("✗ {}", response.message);
            std::process::exit(1);
        }
        
        Ok(())
    }
    
    pub async fn check_connection(socket_path: &str) -> bool {
        let client = IpcClient::new(socket_path.to_string());
        client.ping().await.is_ok()
    }
}