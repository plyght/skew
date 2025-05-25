use crate::config::FocusConfig;
use crate::window_manager::WindowEvent;
use crate::{Result, Window, WindowId};
use log::debug;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration, Instant};

pub struct FocusManager {
    config: FocusConfig,
    event_sender: mpsc::Sender<WindowEvent>,
    last_mouse_move: Option<Instant>,
    last_mouse_pos: (f64, f64),
}

impl FocusManager {
    pub fn new(config: &FocusConfig, event_sender: mpsc::Sender<WindowEvent>) -> Self {
        Self {
            config: config.clone(),
            event_sender,
            last_mouse_move: None,
            last_mouse_pos: (0.0, 0.0),
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        if !self.config.follows_mouse {
            debug!("Focus-follows-mouse disabled in config");
            return Ok(());
        }

        debug!(
            "Starting focus-follows-mouse with {}ms delay",
            self.config.mouse_delay_ms
        );

        let sender = self.event_sender.clone();
        let mouse_delay = Duration::from_millis(self.config.mouse_delay_ms);

        tokio::spawn(async move {
            let mut last_position = (0.0, 0.0);
            let mut last_mouse_event = Instant::now();

            loop {
                sleep(Duration::from_millis(50)).await;

                // Get current mouse position
                let current_position = match Self::get_mouse_position() {
                    Ok(pos) => pos,
                    Err(_) => {
                        // If we can't get mouse position, continue without error
                        continue;
                    }
                };

                // Check if mouse has moved significantly
                if (current_position.0 - last_position.0).abs() > 1.0
                    || (current_position.1 - last_position.1).abs() > 1.0
                {
                    let now = Instant::now();

                    // Apply delay to prevent too frequent updates
                    if now.duration_since(last_mouse_event) >= mouse_delay {
                        let _ = sender
                            .send(WindowEvent::MouseMoved {
                                x: current_position.0,
                                y: current_position.1,
                            })
                            .await;

                        last_mouse_event = now;
                    }

                    last_position = current_position;
                }
            }
        });

        Ok(())
    }

    pub async fn handle_mouse_move(
        &mut self,
        x: f64,
        y: f64,
        windows: &HashMap<WindowId, Window>,
    ) -> Result<()> {
        if !self.config.follows_mouse {
            return Ok(());
        }

        let now = Instant::now();

        // Apply delay filter
        if let Some(last_move) = self.last_mouse_move {
            if now.duration_since(last_move) < Duration::from_millis(self.config.mouse_delay_ms) {
                return Ok(());
            }
        }

        // Check if mouse moved significantly since last event
        if (self.last_mouse_pos.0 - x).abs() < 2.0 && (self.last_mouse_pos.1 - y).abs() < 2.0 {
            return Ok(());
        }

        self.last_mouse_move = Some(now);
        self.last_mouse_pos = (x, y);

        // Find window under cursor
        if let Some(window_id) = self.find_window_at_position(x, y, windows) {
            debug!(
                "Focus follows mouse: focusing window {:?} at ({}, {})",
                window_id, x, y
            );

            // Only send focus event if this isn't already the focused window
            let current_focused = windows.values().find(|w| w.is_focused).map(|w| w.id);
            if current_focused != Some(window_id) {
                let _ = self
                    .event_sender
                    .send(WindowEvent::WindowFocused(window_id))
                    .await;
            }
        }

        Ok(())
    }

    fn find_window_at_position(
        &self,
        x: f64,
        y: f64,
        windows: &HashMap<WindowId, Window>,
    ) -> Option<WindowId> {
        let mut best_match: Option<(WindowId, i32)> = None;

        for (window_id, window) in windows {
            // Skip minimized windows
            if window.is_minimized {
                continue;
            }

            let rect = &window.rect;

            // Check if point is within window bounds
            if x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height {
                // Use a simple layer heuristic - windows with smaller areas are typically on top
                // This is imperfect but works reasonably well
                let area = (rect.width * rect.height) as i32;
                let layer_score = -area; // Negative so smaller windows (higher layer) get higher scores

                match best_match {
                    None => best_match = Some((*window_id, layer_score)),
                    Some((_, best_score)) => {
                        if layer_score > best_score {
                            best_match = Some((*window_id, layer_score));
                        }
                    }
                }
            }
        }

        best_match.map(|(window_id, _)| window_id)
    }

    fn get_mouse_position() -> Result<(f64, f64)> {
        // For now, return a placeholder position - getting actual mouse position
        // requires more complex setup with event taps
        Ok((640.0, 360.0))
    }

    pub fn set_focus_follows_mouse(&mut self, enabled: bool) {
        self.config.follows_mouse = enabled;
        debug!(
            "Focus-follows-mouse {}",
            if enabled { "enabled" } else { "disabled" }
        );
    }

    pub fn set_mouse_delay(&mut self, delay_ms: u64) {
        self.config.mouse_delay_ms = delay_ms;
        debug!("Mouse delay set to {}ms", delay_ms);
    }

    pub fn is_focus_follows_mouse_enabled(&self) -> bool {
        self.config.follows_mouse
    }

    pub fn get_mouse_delay(&self) -> u64 {
        self.config.mouse_delay_ms
    }
}

// Directional focus navigation
#[derive(Debug, Clone, Copy)]
pub enum FocusDirection {
    Left,
    Right,
    Up,
    Down,
}

// Additional utility functions for window focus management
impl FocusManager {
    pub fn find_window_in_direction(
        &self,
        current_window_id: WindowId,
        direction: FocusDirection,
        windows: &HashMap<WindowId, Window>,
    ) -> Option<WindowId> {
        let current_window = windows.get(&current_window_id)?;
        let current_center = (
            current_window.rect.x + current_window.rect.width / 2.0,
            current_window.rect.y + current_window.rect.height / 2.0,
        );

        let mut best_candidate: Option<(WindowId, f64)> = None;

        for (window_id, window) in windows {
            if *window_id == current_window_id || !self.should_focus_window(window) {
                continue;
            }

            let window_center = (
                window.rect.x + window.rect.width / 2.0,
                window.rect.y + window.rect.height / 2.0,
            );

            let is_in_direction = match direction {
                FocusDirection::Left => window_center.0 < current_center.0,
                FocusDirection::Right => window_center.0 > current_center.0,
                FocusDirection::Up => window_center.1 < current_center.1,
                FocusDirection::Down => window_center.1 > current_center.1,
            };

            if !is_in_direction {
                continue;
            }

            // Calculate distance and directional preference
            let dx = window_center.0 - current_center.0;
            let dy = window_center.1 - current_center.1;
            let distance = (dx * dx + dy * dy).sqrt();

            // Apply directional weight to prefer windows more aligned with the direction
            let directional_weight = match direction {
                FocusDirection::Left | FocusDirection::Right => {
                    1.0 + (dy.abs() / dx.abs().max(1.0)) * 0.5
                }
                FocusDirection::Up | FocusDirection::Down => {
                    1.0 + (dx.abs() / dy.abs().max(1.0)) * 0.5
                }
            };

            let weighted_distance = distance * directional_weight;

            match best_candidate {
                None => best_candidate = Some((*window_id, weighted_distance)),
                Some((_, best_distance)) => {
                    if weighted_distance < best_distance {
                        best_candidate = Some((*window_id, weighted_distance));
                    }
                }
            }
        }

        best_candidate.map(|(window_id, _)| window_id)
    }

    pub async fn focus_in_direction(
        &self,
        current_window_id: Option<WindowId>,
        direction: FocusDirection,
        windows: &HashMap<WindowId, Window>,
    ) -> Result<Option<WindowId>> {
        let current_id = match current_window_id {
            Some(id) => id,
            None => {
                // If no current window, find the first focusable window
                let first_window = windows
                    .values()
                    .find(|w| self.should_focus_window(w))
                    .map(|w| w.id);

                if let Some(window_id) = first_window {
                    self.event_sender
                        .send(WindowEvent::WindowFocused(window_id))
                        .await?;
                }

                return Ok(first_window);
            }
        };

        if let Some(target_window_id) =
            self.find_window_in_direction(current_id, direction, windows)
        {
            debug!(
                "Focusing window {:?} in direction {:?}",
                target_window_id, direction
            );
            self.event_sender
                .send(WindowEvent::WindowFocused(target_window_id))
                .await?;
            Ok(Some(target_window_id))
        } else {
            debug!("No window found in direction {:?}", direction);
            Ok(None)
        }
    }

    pub fn get_focused_window_id(&self, windows: &HashMap<WindowId, Window>) -> Option<WindowId> {
        windows
            .iter()
            .find(|(_, window)| window.is_focused)
            .map(|(window_id, _)| *window_id)
    }

    pub fn cycle_focus(
        &self,
        windows: &HashMap<WindowId, Window>,
        forward: bool,
    ) -> Option<WindowId> {
        let focusable_windows: Vec<_> = windows
            .iter()
            .filter(|(_, window)| self.should_focus_window(window))
            .collect();

        if focusable_windows.is_empty() {
            return None;
        }

        let current_focused = focusable_windows
            .iter()
            .position(|(_, window)| window.is_focused);

        let next_index = match current_focused {
            Some(current_index) => {
                if forward {
                    (current_index + 1) % focusable_windows.len()
                } else {
                    if current_index == 0 {
                        focusable_windows.len() - 1
                    } else {
                        current_index - 1
                    }
                }
            }
            None => 0, // No window focused, start with first
        };

        Some(*focusable_windows[next_index].0)
    }
    pub fn should_focus_window(&self, window: &Window) -> bool {
        // Don't focus minimized windows
        if window.is_minimized {
            return false;
        }

        // Don't focus very small windows (likely UI elements)
        if window.rect.width < 100.0 || window.rect.height < 100.0 {
            return false;
        }

        // Don't focus windows with certain titles (system dialogs, etc.)
        if window.title.is_empty()
            || window.title.starts_with("Item-0")
            || window.title == "Desktop"
        {
            return false;
        }

        // Don't focus certain system applications
        if window.owner == "Dock"
            || window.owner == "SystemUIServer"
            || window.owner == "WindowServer"
        {
            return false;
        }

        true
    }

    pub fn get_windows_under_cursor(
        &self,
        x: f64,
        y: f64,
        windows: &HashMap<WindowId, Window>,
    ) -> Vec<WindowId> {
        let mut matching_windows = Vec::new();

        for (window_id, window) in windows {
            if window.is_minimized {
                continue;
            }

            let rect = &window.rect;
            if x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height {
                matching_windows.push(*window_id);
            }
        }

        // Sort by area (smaller windows are likely on top)
        matching_windows.sort_by(|a, b| {
            let area_a = windows[a].rect.width * windows[a].rect.height;
            let area_b = windows[b].rect.width * windows[b].rect.height;
            area_a
                .partial_cmp(&area_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        matching_windows
    }
}
