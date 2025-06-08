use crate::config::HotkeyConfig;
use crate::window_manager::Command;
use crate::Result;
use log::{debug, error, info, warn};
use rdev::{listen, Event, EventType, Key};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use tokio::sync::mpsc;

// Global state for rdev callback - necessary because rdev requires function pointers
static GLOBAL_HOTKEY_SENDER: OnceLock<std::sync::mpsc::Sender<rdev::Event>> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

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
    event_receiver: Option<std::sync::mpsc::Receiver<rdev::Event>>,
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

        // Create channel for rdev events
        let (event_sender, event_receiver) = std::sync::mpsc::channel();

        // Set global sender (this can only be done once)
        if GLOBAL_HOTKEY_SENDER.set(event_sender).is_err() {
            warn!("Global hotkey sender already initialized");
        }

        Ok(Self {
            bindings,
            command_sender,
            pressed_keys: Arc::new(Mutex::new(Vec::new())),
            is_running: Arc::new(Mutex::new(false)),
            event_receiver: Some(event_receiver),
        })
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting hotkey manager with global event listening");

        // List available hotkey bindings
        info!("Configured hotkey bindings:");
        for (combo, action) in &self.bindings {
            info!("  {:?} -> {}", combo, action);
        }

        let mut running = self.is_running.lock().unwrap();
        *running = true;
        drop(running);

        // Take the event receiver (can only be done once)
        let event_receiver = self
            .event_receiver
            .take()
            .ok_or_else(|| anyhow::anyhow!("Event receiver already taken"))?;

        // Clone necessary data for the background tasks
        let bindings = self.bindings.clone();
        let command_sender = self.command_sender.clone();
        let pressed_keys = self.pressed_keys.clone();
        let is_running = self.is_running.clone();

        // Start the rdev listener in a separate thread
        thread::spawn(move || {
            if let Err(e) = listen(global_hotkey_callback) {
                error!("Error in global hotkey listener: {:?}", e);
            }
        });

        // Start the event processing task
        tokio::spawn(async move {
            Self::process_hotkey_events(
                event_receiver,
                bindings,
                command_sender,
                pressed_keys,
                is_running,
            )
            .await;
        });

        info!("Global hotkey listener started successfully");
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

    async fn process_hotkey_events(
        event_receiver: std::sync::mpsc::Receiver<rdev::Event>,
        bindings: HashMap<KeyCombination, String>,
        command_sender: mpsc::Sender<Command>,
        pressed_keys: Arc<Mutex<Vec<Key>>>,
        is_running: Arc<Mutex<bool>>,
    ) {
        info!("Starting hotkey event processing");

        while *is_running.lock().unwrap() {
            // Use a timeout to periodically check if we should stop
            match event_receiver.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok(event) => {
                    if let Err(e) =
                        Self::handle_rdev_event(event, &bindings, &command_sender, &pressed_keys)
                            .await
                    {
                        error!("Error handling hotkey event: {}", e);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // Continue loop to check is_running
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    warn!("Hotkey event channel disconnected");
                    break;
                }
            }
        }

        info!("Hotkey event processing stopped");
    }

    async fn handle_rdev_event(
        event: rdev::Event,
        bindings: &HashMap<KeyCombination, String>,
        command_sender: &mpsc::Sender<Command>,
        pressed_keys: &Arc<Mutex<Vec<Key>>>,
    ) -> Result<()> {
        match event.event_type {
            EventType::KeyPress(key) => {
                debug!("Key pressed: {:?}", key);
                {
                    let mut keys = pressed_keys.lock().unwrap();
                    if !keys
                        .iter()
                        .any(|k| std::mem::discriminant(k) == std::mem::discriminant(&key))
                    {
                        keys.push(key);
                    }
                }

                // Check for matching key combinations
                let keys = pressed_keys.lock().unwrap().clone();
                if let Some(combination) = Self::match_key_combination(&keys, bindings) {
                    info!("Hotkey triggered: {:?}", combination);
                    if let Some(action) = bindings.get(&combination) {
                        let command = Self::parse_action(action)?;
                        if let Err(e) = command_sender.send(command).await {
                            error!("Failed to send command: {}", e);
                        }
                    }
                }
            }
            EventType::KeyRelease(key) => {
                debug!("Key released: {:?}", key);
                {
                    let mut keys = pressed_keys.lock().unwrap();
                    keys.retain(|k| std::mem::discriminant(k) != std::mem::discriminant(&key));
                }
            }
            _ => {} // Ignore other event types
        }

        Ok(())
    }

    fn match_key_combination(
        pressed_keys: &[Key],
        bindings: &HashMap<KeyCombination, String>,
    ) -> Option<KeyCombination> {
        for combination in bindings.keys() {
            if Self::is_combination_pressed(combination, pressed_keys) {
                return Some(combination.clone());
            }
        }
        None
    }

    fn is_combination_pressed(combination: &KeyCombination, pressed_keys: &[Key]) -> bool {
        fn key_is_pressed(keys: &[Key], target: &Key) -> bool {
            keys.iter()
                .any(|k| std::mem::discriminant(k) == std::mem::discriminant(target))
        }
        for modifier in &combination.modifiers {
            match modifier {
                ModifierKey::Alt => {
                    if !key_is_pressed(pressed_keys, &Key::Alt)
                        && !key_is_pressed(pressed_keys, &Key::AltGr)
                    {
                        return false;
                    }
                }
                ModifierKey::Ctrl => {
                    if !key_is_pressed(pressed_keys, &Key::ControlLeft)
                        && !key_is_pressed(pressed_keys, &Key::ControlRight)
                    {
                        return false;
                    }
                }
                ModifierKey::Shift => {
                    if !key_is_pressed(pressed_keys, &Key::ShiftLeft)
                        && !key_is_pressed(pressed_keys, &Key::ShiftRight)
                    {
                        return false;
                    }
                }
                ModifierKey::Cmd => {
                    if !key_is_pressed(pressed_keys, &Key::MetaLeft)
                        && !key_is_pressed(pressed_keys, &Key::MetaRight)
                    {
                        return false;
                    }
                }
            };
        }

        // Check the main key
        if let Some(key) = Self::string_to_key(&combination.key) {
            if !key_is_pressed(pressed_keys, &key) {
                return false;
            }
        } else {
            return false;
        }

        true
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
            "focus_left" => Ok(Command::FocusDirection(Direction::Left)),
            "focus_right" => Ok(Command::FocusDirection(Direction::Right)),
            "focus_up" => Ok(Command::FocusDirection(Direction::Up)),
            "focus_down" => Ok(Command::FocusDirection(Direction::Down)),
            "move_left" => Ok(Command::MoveDirection(Direction::Left)),
            "move_right" => Ok(Command::MoveDirection(Direction::Right)),
            "move_up" => Ok(Command::MoveDirection(Direction::Up)),
            "move_down" => Ok(Command::MoveDirection(Direction::Down)),
            "close_window" => Ok(Command::CloseFocusedWindow),
            "toggle_layout" => Ok(Command::ToggleLayout),
            "toggle_float" => Ok(Command::ToggleFloat),
            "toggle_fullscreen" => Ok(Command::ToggleFullscreen),
            "swap_main" => Ok(Command::SwapMain),
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

// Global callback function for rdev - must be a function pointer
fn global_hotkey_callback(event: Event) {
    if let Some(sender) = GLOBAL_HOTKEY_SENDER.get() {
        if sender.send(event).is_err() {
            // Channel is closed, ignore the error
        }
    }
}
