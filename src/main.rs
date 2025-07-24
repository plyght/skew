use clap::{Parser, Subcommand};
use log::info;
use skew::{Config, Result, WindowManager};
use std::path::PathBuf;
use std::fs::OpenOptions;
use std::io::{Write, BufWriter};
use std::sync::{Arc, Mutex};
use log::{Record, Metadata};

#[derive(Parser)]
#[command(name = "skew")]
#[command(about = "A tiling window manager for macOS")]
struct Cli {
    #[arg(short, long, help = "Configuration file path")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Start the window manager daemon")]
    Start,
    #[command(about = "Stop the window manager daemon")]
    Stop,
    #[command(about = "Reload configuration")]
    Reload,
    #[command(about = "Show window manager status")]
    Status,
}

struct DualLogger {
    file: Arc<Mutex<BufWriter<std::fs::File>>>,
}

impl DualLogger {
    fn new(log_file: &PathBuf) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)?;
        Ok(DualLogger {
            file: Arc::new(Mutex::new(BufWriter::new(file))),
        })
    }
}

impl log::Log for DualLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            let log_line = format!(
                "[{} {} {}:{}] {}\n",
                timestamp,
                record.level(),
                record.file().unwrap_or("unknown"),
                record.line().unwrap_or(0),
                record.args()
            );
            
            // Write to stderr (console)
            eprint!("{}", log_line);
            
            // Write to file
            if let Ok(mut file) = self.file.lock() {
                let _ = file.write_all(log_line.as_bytes());
                let _ = file.flush();
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

fn setup_dual_logging(log_file: &PathBuf) -> Result<()> {
    let dual_logger = DualLogger::new(log_file)?;
    
    log::set_boxed_logger(Box::new(dual_logger))
        .map(|()| log::set_max_level(log::LevelFilter::Debug))
        .map_err(|e| anyhow::anyhow!("Failed to init dual logger: {}", e))?;
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize dual logger (console + file) to show all log levels with timestamps
    let log_file = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("skew.log");
    
    setup_dual_logging(&log_file)?;

    let cli = Cli::parse();

    let config_path = cli.config.unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
            .join(".config")
            .join("skew")
            .join("config.toml")
    });

    match cli.command {
        Some(Commands::Start) | None => {
            info!("Starting Skew window manager");
            let config = Config::load(&config_path)?;
            let mut wm = WindowManager::new(config).await?;
            wm.run().await?;
        }
        Some(Commands::Stop) => {
            info!("Stopping Skew window manager");
            let config = Config::load(&config_path)?;
            if skew::ipc::IpcClient::check_connection(&config.ipc.socket_path).await {
                skew::ipc::IpcClient::run_command(&config.ipc.socket_path, "quit", vec![]).await?;
            } else {
                eprintln!("✗ Daemon is not running");
                std::process::exit(1);
            }
        }
        Some(Commands::Reload) => {
            info!("Reloading configuration");
            let config = Config::load(&config_path)?;
            if skew::ipc::IpcClient::check_connection(&config.ipc.socket_path).await {
                skew::ipc::IpcClient::run_command(&config.ipc.socket_path, "reload", vec![])
                    .await?;
            } else {
                eprintln!("✗ Daemon is not running");
                std::process::exit(1);
            }
        }
        Some(Commands::Status) => {
            info!("Getting window manager status");
            let config = Config::load(&config_path)?;
            if skew::ipc::IpcClient::check_connection(&config.ipc.socket_path).await {
                skew::ipc::IpcClient::run_command(&config.ipc.socket_path, "status", vec![])
                    .await?;
            } else {
                eprintln!("✗ Daemon is not running");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
