use log::{error, info};
use skew::{Config, Result, WindowManager};
use std::path::PathBuf;
use tokio::signal;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    info!("Starting Skew daemon");

    let config_path = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join(".config")
        .join("skew")
        .join("config.toml");

    let config = Config::load(&config_path)?;
    let mut wm = WindowManager::new(config).await?;

    tokio::select! {
        result = wm.run() => {
            if let Err(e) = result {
                error!("Window manager error: {}", e);
            }
        }
        _ = signal::ctrl_c() => {
            info!("Received Ctrl+C, shutting down");
        }
    }

    Ok(())
}
