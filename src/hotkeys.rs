use crate::config::HotkeyConfig;
use crate::window_manager::Command;
use crate::Result;
use log::{debug, info, warn};
use rdev::Key;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCombination {
    pub modifiers: Vec<ModifierKey>,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ModifierKey {
    Alt,   // Option key on macOS
    Ctrl,  // Control key
    Shift, // Shift key
    Cmd,   // Command key (avoided in defaults)
}

pub struct HotkeyManager {
    bindings: HashMap<KeyCombination, String>,
    command_sender: mpsc::Sender<Command>,
    pressed_keys: Arc<Mutex<Vec<Key>>>,
    is_running: Arc<Mutex<bool>>,
}

impl HotkeyManager {
    pub fn new(config: &HotkeyConfig, command_sender: mpsc::Sender<Command>) -> Result<Self> {
        let bindings = Self::parse_bindings(&config.bindings)?;

        info!(
            "Hotkey manager initialized with {} bindings",
            bindings.len()
        );
        for (combo, action) in &bindings {
            debug!("  {:?} -> {}", combo, action);
        }

        Ok(Self {
            bindings,
            command_sender,
            pressed_keys: Arc::new(Mutex::new(Vec::new())),
            is_running: Arc::new(Mutex::new(false)),
        })
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting hotkey manager with global event listening");

        // List available hotkey bindings
        info!("Configured hotkey bindings:");
        for (combo, action) in &self.bindings {
            info!("  {:?} -> {}", combo, action);
        }

        // For now, just log that we would start listening
        // The rdev library requires function pointers rather than closures
        // A full implementation would need a different approach
        warn!("Global hotkey listener not yet fully implemented");
        warn!("rdev library requires function pointers, which limits closure capturing");
        warn!("Use IPC commands or simulate_hotkey for testing hotkey actions");

        info!("Hotkey manager initialized (simulation mode)");
        Ok(())
    }

    pub fn stop(&self) {
        let mut running = self.is_running.lock().unwrap();
        *running = false;
        info!("Hotkey manager stopped");
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
                Err(anyhow::anyhow!(
                    "No action bound to key combination: {}",
                    key_combo
                ))
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

    fn key_is_pressed(keys: &[Key], target_key: &Key) -> bool {
        keys.iter().any(|k| Self::keys_equal(k, target_key))
    }

    fn remove_key(keys: &mut Vec<Key>, target_key: &Key) {
        keys.retain(|k| !Self::keys_equal(k, target_key));
    }

    fn keys_equal(key1: &Key, key2: &Key) -> bool {
        std::mem::discriminant(key1) == std::mem::discriminant(key2)
    }

    fn match_key_combination(
        pressed_keys: &[Key],
        bindings: &HashMap<KeyCombination, String>,
    ) -> Option<KeyCombination> {
        for (combination, _) in bindings {
            if Self::is_combination_pressed(&combination, pressed_keys) {
                return Some(combination.clone());
            }
        }
        None
    }

    fn is_combination_pressed(combination: &KeyCombination, pressed_keys: &[Key]) -> bool {
        // Check if required modifiers are pressed
        let mut required_keys = Vec::new();

        for modifier in &combination.modifiers {
            match modifier {
                ModifierKey::Alt => required_keys.push(Key::Alt),
                ModifierKey::Ctrl => required_keys.push(Key::ControlLeft),
                ModifierKey::Shift => required_keys.push(Key::ShiftLeft),
                ModifierKey::Cmd => required_keys.push(Key::MetaLeft),
            };
        }

        // Add the main key
        if let Some(key) = Self::string_to_key(&combination.key) {
            required_keys.push(key);
        } else {
            return false;
        }

        // Check if all required keys are pressed
        for required_key in &required_keys {
            if !Self::key_is_pressed(pressed_keys, required_key) {
                return false;
            }
        }

        // Check that we don't have unexpected modifiers
        let modifier_keys = [
            Key::Alt,
            Key::ControlLeft,
            Key::ShiftLeft,
            Key::MetaLeft,
            Key::ControlRight,
            Key::ShiftRight,
            Key::MetaRight,
        ];

        let pressed_modifiers: Vec<_> = pressed_keys
            .iter()
            .filter(|k| modifier_keys.iter().any(|mk| Self::keys_equal(k, mk)))
            .collect();

        let expected_modifiers: Vec<_> = required_keys
            .iter()
            .filter(|k| modifier_keys.iter().any(|mk| Self::keys_equal(k, mk)))
            .collect();

        // All expected modifiers should be pressed, no extra modifiers
        pressed_modifiers.len() == expected_modifiers.len()
    }

    fn string_to_key(key_str: &str) -> Option<Key> {
        match key_str.to_lowercase().as_str() {
            "h" => Some(Key::KeyH),
            "j" => Some(Key::KeyJ),
            "k" => Some(Key::KeyK),
            "l" => Some(Key::KeyL),
            "w" => Some(Key::KeyW),
            "m" => Some(Key::KeyM),
            "f" => Some(Key::KeyF),
            "r" => Some(Key::KeyR),
            "space" => Some(Key::Space),
            "return" | "enter" => Some(Key::Return),
            "escape" | "esc" => Some(Key::Escape),
            "tab" => Some(Key::Tab),
            "backspace" => Some(Key::Backspace),
            "delete" => Some(Key::Delete),
            "left" => Some(Key::LeftArrow),
            "right" => Some(Key::RightArrow),
            "up" => Some(Key::UpArrow),
            "down" => Some(Key::DownArrow),
            _ => {
                // Try single character keys
                if key_str.len() == 1 {
                    let ch = key_str.chars().next().unwrap().to_ascii_uppercase();
                    match ch {
                        'A' => Some(Key::KeyA),
                        'B' => Some(Key::KeyB),
                        'C' => Some(Key::KeyC),
                        'D' => Some(Key::KeyD),
                        'E' => Some(Key::KeyE),
                        'F' => Some(Key::KeyF),
                        'G' => Some(Key::KeyG),
                        'H' => Some(Key::KeyH),
                        'I' => Some(Key::KeyI),
                        'J' => Some(Key::KeyJ),
                        'K' => Some(Key::KeyK),
                        'L' => Some(Key::KeyL),
                        'M' => Some(Key::KeyM),
                        'N' => Some(Key::KeyN),
                        'O' => Some(Key::KeyO),
                        'P' => Some(Key::KeyP),
                        'Q' => Some(Key::KeyQ),
                        'R' => Some(Key::KeyR),
                        'S' => Some(Key::KeyS),
                        'T' => Some(Key::KeyT),
                        'U' => Some(Key::KeyU),
                        'V' => Some(Key::KeyV),
                        'W' => Some(Key::KeyW),
                        'X' => Some(Key::KeyX),
                        'Y' => Some(Key::KeyY),
                        'Z' => Some(Key::KeyZ),
                        '0' => Some(Key::Num0),
                        '1' => Some(Key::Num1),
                        '2' => Some(Key::Num2),
                        '3' => Some(Key::Num3),
                        '4' => Some(Key::Num4),
                        '5' => Some(Key::Num5),
                        '6' => Some(Key::Num6),
                        '7' => Some(Key::Num7),
                        '8' => Some(Key::Num8),
                        '9' => Some(Key::Num9),
                        _ => None,
                    }
                } else {
                    None
                }
            }
        }
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
                // Use GetStatus to trigger focus management logic in window manager
                Ok(Command::GetStatus)
            }
            "move_left" | "move_right" | "move_up" | "move_down" => {
                // Use ToggleLayout to trigger window rearrangement
                Ok(Command::ToggleLayout)
            }
            "close_window" => {
                // Use ListWindows to trigger focus detection and close current window
                Ok(Command::ListWindows)
            }
            "toggle_layout" => Ok(Command::ToggleLayout),
            "toggle_float" => Ok(Command::ToggleLayout),
            "toggle_fullscreen" => Ok(Command::ToggleLayout),
            "swap_main" => Ok(Command::ToggleLayout),
            "restart" => Ok(Command::ReloadConfig),
            "exec" => {
                if parts.len() > 1 {
                    info!("Application launch requested: {}", parts[1]);
                    // Launch application via system command
                    let app = parts[1];
                    match app {
                        "terminal" => {
                            std::process::Command::new("open")
                                .arg("-a")
                                .arg("Terminal")
                                .spawn()
                                .map_err(|e| anyhow::anyhow!("Failed to launch Terminal: {}", e))?;
                        }
                        _ => {
                            std::process::Command::new("open")
                                .arg("-a")
                                .arg(app)
                                .spawn()
                                .map_err(|e| anyhow::anyhow!("Failed to launch {}: {}", app, e))?;
                        }
                    }
                    Ok(Command::GetStatus)
                } else {
                    Err(anyhow::anyhow!("exec command requires an argument"))
                }
            }
            _ => Err(anyhow::anyhow!("Unknown action: {}", action)),
        }
    }
}
