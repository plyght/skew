use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub general: GeneralConfig,
    pub layout: LayoutConfig,
    pub focus: FocusConfig,
    pub hotkeys: HotkeyConfig,
    pub ipc: IpcConfig,
    pub plugins: PluginConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_gap")]
    pub gap: f64,
    #[serde(default = "default_border_width")]
    pub border_width: f64,
    #[serde(default = "default_border_color")]
    pub border_color: String,
    #[serde(default = "default_active_border_color")]
    pub active_border_color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutConfig {
    #[serde(default = "default_layout_type")]
    pub default_layout: String,
    #[serde(default = "default_split_ratio")]
    pub split_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusConfig {
    #[serde(default = "default_focus_follows_mouse")]
    pub follows_mouse: bool,
    #[serde(default = "default_mouse_delay")]
    pub mouse_delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub mod_key: String,
    pub bindings: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcConfig {
    #[serde(default = "default_socket_path")]
    pub socket_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    #[serde(default)]
    pub enabled: Vec<String>,
    #[serde(default = "default_plugin_dir")]
    pub plugin_dir: String,
}

fn default_gap() -> f64 {
    10.0
}
fn default_border_width() -> f64 {
    2.0
}
fn default_border_color() -> String {
    "#cccccc".to_string()
}
fn default_active_border_color() -> String {
    "#0080ff".to_string()
}
fn default_layout_type() -> String {
    "bsp".to_string()
}
fn default_split_ratio() -> f64 {
    0.5
}
fn default_focus_follows_mouse() -> bool {
    true
}
fn default_mouse_delay() -> u64 {
    100
}
fn default_socket_path() -> String {
    "/tmp/skew.sock".to_string()
}
fn default_plugin_dir() -> String {
    format!(
        "{}/.config/skew/plugins",
        std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
    )
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig {
                gap: default_gap(),
                border_width: default_border_width(),
                border_color: default_border_color(),
                active_border_color: default_active_border_color(),
            },
            layout: LayoutConfig {
                default_layout: default_layout_type(),
                split_ratio: default_split_ratio(),
            },
            focus: FocusConfig {
                follows_mouse: default_focus_follows_mouse(),
                mouse_delay_ms: default_mouse_delay(),
            },
            hotkeys: HotkeyConfig {
                mod_key: "alt".to_string(),
                bindings: default_hotkeys(),
            },
            ipc: IpcConfig {
                socket_path: default_socket_path(),
            },
            plugins: PluginConfig {
                enabled: vec![],
                plugin_dir: default_plugin_dir(),
            },
        }
    }
}

fn default_hotkeys() -> std::collections::HashMap<String, String> {
    let mut bindings = std::collections::HashMap::new();
    // Primary navigation - alt + hjkl (vim-style)
    bindings.insert("alt+h".to_string(), "focus_left".to_string());
    bindings.insert("alt+j".to_string(), "focus_down".to_string());
    bindings.insert("alt+k".to_string(), "focus_up".to_string());
    bindings.insert("alt+l".to_string(), "focus_right".to_string());
    
    // Window movement - alt + shift + hjkl
    bindings.insert("alt+shift+h".to_string(), "move_left".to_string());
    bindings.insert("alt+shift+j".to_string(), "move_down".to_string());
    bindings.insert("alt+shift+k".to_string(), "move_up".to_string());
    bindings.insert("alt+shift+l".to_string(), "move_right".to_string());
    
    // Layout controls - ctrl + alt combinations
    bindings.insert("ctrl+alt+space".to_string(), "toggle_layout".to_string());
    bindings.insert("ctrl+alt+f".to_string(), "toggle_float".to_string());
    bindings.insert("ctrl+alt+r".to_string(), "rotate_layout".to_string());
    
    // Window actions - alt + action keys
    bindings.insert("alt+return".to_string(), "exec:terminal".to_string());
    bindings.insert("alt+w".to_string(), "close_window".to_string());
    bindings.insert("alt+m".to_string(), "toggle_fullscreen".to_string());
    
    // Advanced - alt + shift + action
    bindings.insert("alt+shift+space".to_string(), "swap_main".to_string());
    bindings.insert("alt+shift+r".to_string(), "restart".to_string());
    
    bindings
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();

        if !path.exists() {
            log::info!("Config file not found at {:?}, using defaults", path);
            let config = Self::default();
            config.save(path)?;
            return Ok(config);
        }

        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let path = path.as_ref();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn reload<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        *self = Self::load(path)?;
        Ok(())
    }
}
