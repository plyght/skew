use super::accessibility::AccessibilityManager;
use super::cgwindow::CGWindowInfo;
use crate::window_manager::WindowEvent;
use crate::{Rect, Result, Window, WindowId};
use core_graphics::display::{CGDisplayBounds, CGGetActiveDisplayList, CGMainDisplayID};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

// macOS workspace detection
extern "C" {
    fn CGSGetActiveSpace(connection: u32) -> u32;
    fn CGSMainConnectionID() -> u32;
}

#[derive(Debug, Clone)]
pub struct Display {
    pub id: u32,
    pub rect: Rect,
    pub is_main: bool,
    pub name: String,
}

pub struct MacOSWindowSystem {
    accessibility: AccessibilityManager,
    event_sender: mpsc::Sender<WindowEvent>,
    displays: HashMap<u32, Display>,
}

impl MacOSWindowSystem {
    pub async fn new(event_sender: mpsc::Sender<WindowEvent>) -> Result<Self> {
        let accessibility = AccessibilityManager::new()?;
        let displays = Self::get_all_displays()?;

        Ok(Self {
            accessibility,
            event_sender,
            displays,
        })
    }

    fn get_all_displays() -> Result<HashMap<u32, Display>> {
        unsafe {
            let mut display_count: u32 = 0;
            let mut display_list: Vec<u32> = vec![0; 32]; // Max 32 displays

            let result = CGGetActiveDisplayList(
                display_list.len() as u32,
                display_list.as_mut_ptr(),
                &mut display_count,
            );

            if result != 0 {
                warn!("Failed to get display list, using main display only");
                // Fall back to main display only
                let main_display_id = CGMainDisplayID();
                let bounds = CGDisplayBounds(main_display_id);
                let mut displays = HashMap::new();
                displays.insert(
                    main_display_id,
                    Display {
                        id: main_display_id,
                        rect: Rect::new(
                            bounds.origin.x,
                            bounds.origin.y,
                            bounds.size.width,
                            bounds.size.height,
                        ),
                        is_main: true,
                        name: "Main Display".to_string(),
                    },
                );
                return Ok(displays);
            }

            let main_display_id = CGMainDisplayID();
            let mut displays = HashMap::new();

            info!("Found {} display(s)", display_count);

            for (i, &display_id) in display_list.iter().enumerate().take(display_count as usize) {

                let bounds = CGDisplayBounds(display_id);
                let is_main = display_id == main_display_id;

                let display = Display {
                    id: display_id,
                    rect: Rect::new(
                        bounds.origin.x,
                        bounds.origin.y,
                        bounds.size.width,
                        bounds.size.height,
                    ),
                    is_main,
                    name: if is_main {
                        "Main Display".to_string()
                    } else {
                        format!("Display {}", i + 1)
                    },
                };

                info!(
                    "Display {}: {}x{} at ({}, {}) - {}",
                    display_id,
                    display.rect.width,
                    display.rect.height,
                    display.rect.x,
                    display.rect.y,
                    if display.is_main { "Main" } else { "Secondary" }
                );

                displays.insert(display_id, display);
            }

            Ok(displays)
        }
    }

    pub async fn start_monitoring(&self) -> Result<()> {
        debug!("Starting window monitoring");

        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            // Window monitoring at 200ms provides responsive detection of window changes
            // TODO: Make this configurable via skew.toml with key 'window_monitor_interval_ms'
            // Recommended range: 100-500ms (lower = more responsive, higher = less CPU usage)
            // Note: This interval should be configurable in production as it can be
            // performance-intensive with CGWindowListCopyWindowInfo calls
            let mut interval = interval(Duration::from_millis(200));
            let mut last_windows = Vec::new();

            loop {
                interval.tick().await;

                match CGWindowInfo::get_all_windows() {
                    Ok(current_windows) => {
                        debug!("Window scan found {} windows", current_windows.len());
                        for window in &current_windows {
                            debug!(
                                "Window: {} ({}), workspace: {}, rect: {:?}",
                                window.title, window.owner, window.workspace_id, window.rect
                            );
                        }
                        Self::detect_window_changes(&sender, &last_windows, &current_windows).await;
                        last_windows = current_windows;
                    }
                    Err(e) => {
                        error!("Failed to get window list: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    async fn detect_window_changes(
        sender: &mpsc::Sender<WindowEvent>,
        old_windows: &[Window],
        new_windows: &[Window],
    ) {
        for new_window in new_windows {
            if !old_windows.iter().any(|w| w.id == new_window.id) {
                debug!(
                    "New window detected: {} ({})",
                    new_window.title, new_window.owner
                );
                let _ = sender
                    .send(WindowEvent::WindowCreated(new_window.clone()))
                    .await;
            } else if let Some(old_window) = old_windows.iter().find(|w| w.id == new_window.id) {
                if old_window.rect.x != new_window.rect.x
                    || old_window.rect.y != new_window.rect.y
                    || old_window.rect.width != new_window.rect.width
                    || old_window.rect.height != new_window.rect.height
                {
                    let _ = sender
                        .send(WindowEvent::WindowMoved(
                            new_window.id,
                            new_window.rect,
                        ))
                        .await;
                }
            }
        }

        for old_window in old_windows {
            if !new_windows.iter().any(|w| w.id == old_window.id) {
                let _ = sender
                    .send(WindowEvent::WindowDestroyed(old_window.id))
                    .await;
            }
        }
    }

    pub async fn get_windows(&self) -> Result<Vec<Window>> {
        CGWindowInfo::get_all_windows()
    }

    pub async fn get_screen_rect(&self) -> Result<Rect> {
        // Return main display rect for backward compatibility
        self.get_main_display_rect()
    }

    pub fn get_main_display_rect(&self) -> Result<Rect> {
        let main_display = self
            .displays
            .values()
            .find(|d| d.is_main)
            .ok_or_else(|| anyhow::anyhow!("No main display found"))?;
        Ok(main_display.rect)
    }

    pub fn get_displays(&self) -> &HashMap<u32, Display> {
        &self.displays
    }

    pub fn get_display_for_window(&self, window: &Window) -> Option<&Display> {
        // Find which display contains the center of the window
        let window_center_x = window.rect.x + window.rect.width / 2.0;
        let window_center_y = window.rect.y + window.rect.height / 2.0;

        self.displays.values().find(|display| {
            window_center_x >= display.rect.x
                && window_center_x < display.rect.x + display.rect.width
                && window_center_y >= display.rect.y
                && window_center_y < display.rect.y + display.rect.height
        })
    }

    pub fn get_display_by_id(&self, display_id: u32) -> Option<&Display> {
        self.displays.get(&display_id)
    }

    pub fn get_windows_by_display<'a>(
        &self,
        windows: &'a [Window],
    ) -> HashMap<u32, Vec<&'a Window>> {
        let mut windows_by_display: HashMap<u32, Vec<&'a Window>> = HashMap::new();

        // Initialize empty vectors for each display
        for display_id in self.displays.keys() {
            windows_by_display.insert(*display_id, Vec::new());
        }

        // Assign windows to displays
        for window in windows {
            if let Some(display) = self.get_display_for_window(window) {
                windows_by_display
                    .get_mut(&display.id)
                    .unwrap()
                    .push(window);
            } else {
                // If window doesn't clearly belong to any display, assign to main display
                if let Some(main_display) = self.displays.values().find(|d| d.is_main) {
                    windows_by_display
                        .get_mut(&main_display.id)
                        .unwrap()
                        .push(window);
                }
            }
        }

        windows_by_display
    }

    pub async fn move_window_to_display(
        &mut self,
        window_id: WindowId,
        target_display_id: u32,
    ) -> Result<()> {
        if let Some(target_display) = self.displays.get(&target_display_id) {
            // Calculate new position centered on the target display
            let new_x = target_display.rect.x + target_display.rect.width * 0.1;
            let new_y = target_display.rect.y + target_display.rect.height * 0.1;
            let new_width = target_display.rect.width * 0.8;
            let new_height = target_display.rect.height * 0.8;

            let new_rect = Rect::new(new_x, new_y, new_width, new_height);
            self.move_window(window_id, new_rect).await
        } else {
            Err(anyhow::anyhow!("Display {} not found", target_display_id))
        }
    }

    pub fn refresh_displays(&mut self) -> Result<()> {
        self.displays = Self::get_all_displays()?;
        info!(
            "Display configuration refreshed - {} display(s) detected",
            self.displays.len()
        );
        Ok(())
    }

    pub async fn focus_window(&mut self, window_id: WindowId) -> Result<()> {
        self.accessibility.focus_window(window_id)
    }

    pub async fn move_window(&mut self, window_id: WindowId, rect: Rect) -> Result<()> {
        self.accessibility.move_window(window_id, rect)
    }

    pub async fn move_all_windows(
        &mut self,
        layouts: &std::collections::HashMap<WindowId, Rect>,
        windows: &[crate::Window],
    ) -> Result<()> {
        self.accessibility.move_all_windows(layouts, windows)
    }

    pub async fn close_window(&mut self, window_id: WindowId) -> Result<()> {
        self.accessibility.close_window(window_id)
    }

    pub async fn get_focused_window(&self) -> Result<Option<WindowId>> {
        self.accessibility.get_focused_window()
    }

    pub async fn get_current_workspace(&self) -> Result<u32> {
        unsafe {
            let connection = CGSMainConnectionID();
            if connection == 0 {
                return Err(anyhow::anyhow!("Failed to get main connection ID"));
            }

            let workspace = CGSGetActiveSpace(connection);
            if workspace == 0 {
                // SAFETY: CGSGetActiveSpace can return 0 on failure or when the system
                // is in an inconsistent state. Falling back to workspace 1 provides
                // a reasonable default that allows the window manager to continue
                // functioning, as workspace 1 typically represents the first/main desktop.
                // This fallback prevents crashes while maintaining basic functionality.
                warn!("CGSGetActiveSpace returned 0, falling back to workspace 1");
                debug!("Workspace fallback reason: CGS API returned invalid workspace ID");
                Ok(1)
            } else {
                Ok(workspace)
            }
        }
    }
}
