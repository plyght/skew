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
    bindings.insert("ctrl+alt+r".to_string(), "toggle_layout".to_string());

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
            config.validate()?;
            config.save(path)?;
            return Ok(config);
        }

        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;

        // Validate the loaded configuration
        config.validate().map_err(|e| {
            anyhow::anyhow!(
                "Configuration validation failed for '{}': {}",
                path.display(),
                e
            )
        })?;

        log::info!("Configuration loaded and validated from {:?}", path);
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

    pub fn validate(&self) -> Result<()> {
        self.general.validate()?;
        self.layout.validate()?;
        self.focus.validate()?;
        self.hotkeys.validate()?;
        self.ipc.validate()?;
        self.plugins.validate()?;
        Ok(())
    }
}

impl GeneralConfig {
    pub fn validate(&self) -> Result<()> {
        if self.gap < 0.0 || self.gap > 100.0 {
            return Err(anyhow::anyhow!(
                "gap must be between 0 and 100, got {}",
                self.gap
            ));
        }

        if self.border_width < 0.0 || self.border_width > 20.0 {
            return Err(anyhow::anyhow!(
                "border_width must be between 0 and 20, got {}",
                self.border_width
            ));
        }

        if !self.border_color.starts_with('#') || self.border_color.len() != 7 {
            return Err(anyhow::anyhow!(
                "border_color must be a valid hex color (e.g., #ff0000), got {}",
                self.border_color
            ));
        }

        if !self.active_border_color.starts_with('#') || self.active_border_color.len() != 7 {
            return Err(anyhow::anyhow!(
                "active_border_color must be a valid hex color (e.g., #ff0000), got {}",
                self.active_border_color
            ));
        }

        Ok(())
    }
}

impl LayoutConfig {
    pub fn validate(&self) -> Result<()> {
        let valid_layouts = [
            "bsp", "stack", "float", "grid", "spiral", "column", "monocle",
        ];
        if !valid_layouts.contains(&self.default_layout.to_lowercase().as_str()) {
            return Err(anyhow::anyhow!(
                "default_layout must be one of {:?}, got '{}'",
                valid_layouts,
                self.default_layout
            ));
        }

        if self.split_ratio <= 0.0 || self.split_ratio >= 1.0 {
            return Err(anyhow::anyhow!(
                "split_ratio must be between 0 and 1, got {}",
                self.split_ratio
            ));
        }

        Ok(())
    }
}

impl FocusConfig {
    pub fn validate(&self) -> Result<()> {
        if self.mouse_delay_ms > 10000 {
            return Err(anyhow::anyhow!(
                "mouse_delay_ms should not exceed 10000ms, got {}",
                self.mouse_delay_ms
            ));
        }

        Ok(())
    }
}

impl HotkeyConfig {
    pub fn validate(&self) -> Result<()> {
        let valid_modifiers = [
            "alt", "option", "ctrl", "control", "shift", "cmd", "command",
        ];
        if !valid_modifiers.contains(&self.mod_key.to_lowercase().as_str()) {
            return Err(anyhow::anyhow!(
                "mod_key must be one of {:?}, got '{}'",
                valid_modifiers,
                self.mod_key
            ));
        }

        // Validate hotkey bindings format
        for (key_combo, action) in &self.bindings {
            // Check key combination format
            if key_combo.is_empty() {
                return Err(anyhow::anyhow!("Empty key combination not allowed"));
            }

            let parts: Vec<&str> = key_combo.split('+').collect();
            if parts.len() < 1 {
                return Err(anyhow::anyhow!(
                    "Invalid key combination format: '{}'",
                    key_combo
                ));
            }

            // Validate modifiers in the key combination
            for part in &parts[..parts.len().saturating_sub(1)] {
                if !valid_modifiers.contains(&part.to_lowercase().as_str()) {
                    return Err(anyhow::anyhow!(
                        "Invalid modifier '{}' in key combination '{}'",
                        part,
                        key_combo
                    ));
                }
            }

            // Validate action format
            if action.is_empty() {
                return Err(anyhow::anyhow!(
                    "Empty action not allowed for key combination '{}'",
                    key_combo
                ));
            }

            let action_parts: Vec<&str> = action.split(':').collect();
            let action_name = action_parts[0];
            let valid_actions = [
                "focus_left",
                "focus_right",
                "focus_up",
                "focus_down",
                "move_left",
                "move_right",
                "move_up",
                "move_down",
                "close_window",
                "toggle_layout",
                "toggle_float",
                "toggle_fullscreen",
                "swap_main",
                "restart",
                "exec",
            ];

            if !valid_actions.contains(&action_name) {
                return Err(anyhow::anyhow!(
                    "Invalid action '{}' in binding '{}'. Valid actions: {:?}",
                    action_name,
                    key_combo,
                    valid_actions
                ));
            }

            // Special validation for exec actions
            if action_name == "exec" && action_parts.len() < 2 {
                return Err(anyhow::anyhow!(
                    "exec action requires an argument: '{}'",
                    action
                ));
            }
        }

        Ok(())
    }
}

impl IpcConfig {
    pub fn validate(&self) -> Result<()> {
        if self.socket_path.is_empty() {
            return Err(anyhow::anyhow!("socket_path cannot be empty"));
        }

        // Check if parent directory exists or can be created
        if let Some(parent) = std::path::Path::new(&self.socket_path).parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    anyhow::anyhow!(
                        "Cannot create socket directory '{}': {}",
                        parent.display(),
                        e
                    )
                })?;
            }
        }

        Ok(())
    }
}

impl PluginConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.plugin_dir.is_empty() {
            let plugin_path = std::path::Path::new(&self.plugin_dir);
            if !plugin_path.exists() {
                std::fs::create_dir_all(plugin_path).map_err(|e| {
                    anyhow::anyhow!(
                        "Cannot create plugin directory '{}': {}",
                        self.plugin_dir,
                        e
                    )
                })?;
                log::info!("Created plugin directory at '{}'", self.plugin_dir);
            }

            if plugin_path.exists() && !plugin_path.is_dir() {
                return Err(anyhow::anyhow!(
                    "plugin_dir '{}' is not a directory",
                    self.plugin_dir
                ));
            }
        }

        // Validate that enabled plugins exist
        if !self.plugin_dir.is_empty() {
            for plugin_name in &self.enabled {
                let plugin_path =
                    std::path::Path::new(&self.plugin_dir).join(format!("{}.lua", plugin_name));
                if !plugin_path.exists() {
                    return Err(anyhow::anyhow!(
                        "Plugin '{}' not found at '{}'",
                        plugin_name,
                        plugin_path.display()
                    ));
                }
            }
        }

        Ok(())
    }
}
