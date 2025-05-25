use crate::config::HotkeyConfig;
use crate::window_manager::Command;
use crate::{Result, WindowId};
use log::{debug, info, warn};
use std::collections::HashMap;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCombination {
    pub modifiers: Vec<ModifierKey>,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ModifierKey {
    Alt,    // Option key on macOS
    Ctrl,   // Control key
    Shift,  // Shift key
    Cmd,    // Command key (avoided in defaults)
}

pub struct HotkeyManager {
    bindings: HashMap<KeyCombination, String>,
    command_sender: mpsc::Sender<Command>,
}

impl HotkeyManager {
    pub fn new(config: &HotkeyConfig, command_sender: mpsc::Sender<Command>) -> Result<Self> {
        let bindings = Self::parse_bindings(&config.bindings)?;
        
        info!("Hotkey manager initialized with {} bindings", bindings.len());
        for (combo, action) in &bindings {
            debug!("  {:?} -> {}", combo, action);
        }

        Ok(Self {
            bindings,
            command_sender,
        })
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting hotkey manager");
        warn!("Global hotkey registration not yet implemented");
        warn!("Real implementation requires Carbon Event Manager or newer macOS APIs");
        warn!("For testing, use IPC commands to trigger actions");
        
        // List available hotkey bindings
        info!("Configured hotkey bindings:");
        for (combo, action) in &self.bindings {
            info!("  {:?} -> {}", combo, action);
        }
        
        Ok(())
    }
    
    pub fn reload_bindings(&mut self, config: &HotkeyConfig) -> Result<()> {
        info!("Reloading hotkey bindings");
        self.bindings = Self::parse_bindings(&config.bindings)?;
        info!("Reloaded {} hotkey bindings", self.bindings.len());
        Ok(())
    }
    
    pub fn get_bindings(&self) -> &HashMap<KeyCombination, String> {
        &self.bindings
    }
    
    // Simulate a hotkey trigger for testing
    pub async fn simulate_hotkey(&self, key_combo: &str) -> Result<()> {
        if let Some(combination) = Self::parse_key_combination(key_combo) {
            if let Some(action) = self.bindings.get(&combination) {
                debug!("Simulating hotkey: {:?} -> {}", combination, action);
                let command = Self::parse_action(action)?;
                self.command_sender.send(command).await?;
                Ok(())
            } else {
                Err(anyhow::anyhow!("No action bound to key combination: {}", key_combo))
            }
        } else {
            Err(anyhow::anyhow!("Invalid key combination: {}", key_combo))
        }
    }
    
    fn parse_bindings(
        config_bindings: &HashMap<String, String>,
    ) -> Result<HashMap<KeyCombination, String>> {
        let mut bindings = HashMap::new();

        for (key_combo, action) in config_bindings {
            match Self::parse_key_combination(key_combo) {
                Some(combination) => {
                    bindings.insert(combination, action.clone());
                }
                None => {
                    warn!("Failed to parse key combination: {}", key_combo);
                }
            }
        }

        Ok(bindings)
    }

    fn parse_key_combination(combo: &str) -> Option<KeyCombination> {
        let parts: Vec<&str> = combo.split('+').collect();
        if parts.is_empty() {
            return None;
        }

        let mut modifiers = Vec::new();
        let key_str = parts.last()?;

        for part in &parts[..parts.len() - 1] {
            match part.to_lowercase().as_str() {
                "alt" | "option" => modifiers.push(ModifierKey::Alt),
                "ctrl" | "control" => modifiers.push(ModifierKey::Ctrl),
                "shift" => modifiers.push(ModifierKey::Shift),
                "cmd" | "command" => modifiers.push(ModifierKey::Cmd),
                _ => {
                    warn!("Unknown modifier key: {}", part);
                    return None;
                }
            }
        }

        Some(KeyCombination {
            modifiers,
            key: key_str.to_string(),
        })
    }
    
    fn parse_action(action: &str) -> Result<Command> {
        let parts: Vec<&str> = action.split(':').collect();
        let command = parts[0];

        match command {
            "focus_left" | "focus_right" | "focus_up" | "focus_down" => {
                // TODO: implement proper directional focus with current window
                Ok(Command::FocusWindow(WindowId(2))) // Focus Safari for demo
            }
            "move_left" | "move_right" | "move_up" | "move_down" => {
                // TODO: implement proper window movement
                Ok(Command::ToggleLayout) // Toggle layout for demo
            }
            "close_window" => {
                // TODO: get current focused window
                Ok(Command::CloseWindow(WindowId(2))) // Close Safari for demo
            }
            "toggle_layout" => Ok(Command::ToggleLayout),
            "toggle_float" => Ok(Command::ToggleLayout),
            "toggle_fullscreen" => Ok(Command::ToggleLayout),
            "swap_main" => Ok(Command::ToggleLayout),
            "restart" => Ok(Command::ReloadConfig),
            "exec" => {
                if parts.len() > 1 {
                    // TODO: implement application launching
                    info!("Would execute: {}", parts[1]);
                    Ok(Command::GetStatus)
                } else {
                    Err(anyhow::anyhow!("exec command requires an argument"))
                }
            }
            _ => Err(anyhow::anyhow!("Unknown action: {}", action)),
        }
    }
}