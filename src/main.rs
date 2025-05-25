use clap::{Parser, Subcommand};
use log::{error, info};
use skew::{Config, Result, WindowManager};
use std::path::PathBuf;

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

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

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
            // TODO: Send stop command via IPC
        }
        Some(Commands::Reload) => {
            info!("Reloading configuration");
            // TODO: Send reload command via IPC
        }
        Some(Commands::Status) => {
            info!("Getting window manager status");
            // TODO: Query status via IPC
        }
    }

    Ok(())
}
