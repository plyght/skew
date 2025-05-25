use super::accessibility::AccessibilityManager;
use super::cgwindow::CGWindowInfo;
use crate::window_manager::WindowEvent;
use crate::{Rect, Result, Window, WindowId};
use core_graphics::display::{CGDisplayBounds, CGMainDisplayID};
use log::{debug, error, warn};
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

pub struct MacOSWindowSystem {
    accessibility: AccessibilityManager,
    event_sender: mpsc::Sender<WindowEvent>,
}

impl MacOSWindowSystem {
    pub async fn new(event_sender: mpsc::Sender<WindowEvent>) -> Result<Self> {
        let accessibility = AccessibilityManager::new()?;

        Ok(Self {
            accessibility,
            event_sender,
        })
    }

    pub async fn start_monitoring(&self) -> Result<()> {
        debug!("Starting window monitoring");

        let sender = self.event_sender.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_millis(500));
            let mut last_windows = Vec::new();

            loop {
                interval.tick().await;

                match CGWindowInfo::get_all_windows() {
                    Ok(current_windows) => {
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
                            new_window.rect.clone(),
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
        let display_id = unsafe { CGMainDisplayID() };
        let bounds = unsafe { CGDisplayBounds(display_id) };

        Ok(Rect::new(
            bounds.origin.x,
            bounds.origin.y,
            bounds.size.width,
            bounds.size.height,
        ))
    }

    pub async fn focus_window(&self, window_id: WindowId) -> Result<()> {
        self.accessibility.focus_window(window_id)
    }

    pub async fn move_window(&self, window_id: WindowId, rect: Rect) -> Result<()> {
        self.accessibility.move_window(window_id, rect)
    }

    pub async fn close_window(&self, window_id: WindowId) -> Result<()> {
        self.accessibility.close_window(window_id)
    }

    pub async fn get_focused_window(&self) -> Result<Option<WindowId>> {
        self.accessibility.get_focused_window()
    }
}
