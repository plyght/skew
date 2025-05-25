pub mod config;
pub mod focus;
pub mod hotkeys;
pub mod ipc;
pub mod layout;
pub mod macos;
pub mod plugins;
pub mod window_manager;

pub use config::Config;
pub use window_manager::{Window, WindowManager};

pub type Result<T> = anyhow::Result<T>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub u32);

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}
