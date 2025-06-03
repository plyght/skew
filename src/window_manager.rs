use crate::focus::FocusManager;
use crate::hotkeys::HotkeyManager;
use crate::ipc::IpcServer;
use crate::layout::LayoutManager;
use crate::macos::MacOSWindowSystem;
use crate::plugins::PluginManager;
use crate::snap::SnapManager;
use crate::{Config, Rect, Result, WindowId};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

#[derive(Debug, Clone)]
pub struct Window {
    pub id: WindowId,
    pub title: String,
    pub owner: String,
    pub owner_pid: i32,
    pub rect: Rect,
    pub is_minimized: bool,
    pub is_focused: bool,
    pub workspace_id: u32,
}

#[derive(Debug)]
pub enum WindowEvent {
    WindowCreated(Window),
    WindowDestroyed(WindowId),
    WindowMoved(WindowId, Rect),
    WindowResized(WindowId, Rect),
    WindowFocused(WindowId),
    WindowMinimized(WindowId),
    WindowUnminimized(WindowId),
    WorkspaceChanged(u32),
    MouseMoved { x: f64, y: f64 },
}

#[derive(Debug)]
pub enum Command {
    FocusWindow(WindowId),
    FocusDirection(crate::hotkeys::Direction),
    MoveDirection(crate::hotkeys::Direction),
    CloseWindow(WindowId),
    CloseFocusedWindow,
    MoveWindow(WindowId, Rect),
    ToggleLayout,
    ToggleFloat,
    ToggleFullscreen,
    SwapMain,
    ReloadConfig,
    ListWindows,
    GetStatus,
    Quit,
    CheckSnapDragEnd(WindowId, Rect),
}

pub struct WindowManager {
    config: Config,
    windows: HashMap<WindowId, Window>,
    current_workspace: u32,

    macos: MacOSWindowSystem,
    layout_manager: LayoutManager,
    focus_manager: FocusManager,
    ipc_server: IpcServer,
    hotkey_manager: HotkeyManager,
    plugin_manager: PluginManager,
    snap_manager: SnapManager,

    event_rx: mpsc::Receiver<WindowEvent>,
    command_rx: mpsc::Receiver<Command>,
    command_tx: mpsc::Sender<Command>,
}

impl WindowManager {
    pub async fn new(config: Config) -> Result<Self> {
        let (event_tx, event_rx) = mpsc::channel(1000);
        let (command_tx, command_rx) = mpsc::channel(1000);

        let macos = MacOSWindowSystem::new(event_tx.clone()).await?;
        let layout_manager = LayoutManager::new(&config.layout);
        let focus_manager = FocusManager::new(&config.focus, event_tx.clone());
        let ipc_server = IpcServer::new(&config.ipc, command_tx.clone()).await?;
        let hotkey_manager = HotkeyManager::new(&config.hotkeys, command_tx.clone())?;
        let plugin_manager = PluginManager::new(&config.plugins)?;

        // Initialize snap manager with screen rect
        let screen_rect = macos.get_screen_rect().await?;
        let snap_manager = SnapManager::new(screen_rect, 50.0); // 50px snap threshold

        Ok(Self {
            config,
            windows: HashMap::new(),
            current_workspace: 1,
            macos,
            layout_manager,
            focus_manager,
            ipc_server,
            hotkey_manager,
            plugin_manager,
            snap_manager,
            event_rx,
            command_rx,
            command_tx,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        info!("Starting window manager event loop");

        self.macos.start_monitoring().await?;
        self.focus_manager.start().await?;
        self.ipc_server.start().await?;
        self.hotkey_manager.start().await?;

        // Apply layout to existing windows on startup
        info!("Applying initial layout to existing windows...");
        self.refresh_windows().await?;
        self.apply_layout().await?;
        info!("Initial layout application completed");

        // Refresh timer runs every 1000ms to periodically sync window state,
        // while window monitoring runs at 200ms for responsiveness.
        // TODO: Make both intervals configurable via skew.toml:
        //   - 'window_refresh_interval_ms' (current: 1000ms, recommended: 500-2000ms)
        //   - 'window_monitor_interval_ms' (current: 200ms, recommended: 100-500ms)
        // The slower refresh prevents excessive API calls while maintaining accuracy.
        let mut refresh_timer = interval(Duration::from_millis(1000));

        loop {
            tokio::select! {
                Some(event) = self.event_rx.recv() => {
                    if let Err(e) = self.handle_window_event(event).await {
                        error!("Error handling window event: {}", e);
                    }
                }
                Some(command) = self.command_rx.recv() => {
                    if let Err(e) = self.handle_command(command).await {
                        error!("Error handling command: {}", e);
                    }
                }
                _ = refresh_timer.tick() => {
                    if let Err(e) = self.refresh_windows().await {
                        error!("Error refreshing windows: {}", e);
                    }
                }
            }
        }
    }

    async fn handle_window_event(&mut self, event: WindowEvent) -> Result<()> {
        debug!("Handling window event: {:?}", event);

        match event {
            WindowEvent::WindowCreated(window) => {
                self.windows.insert(window.id, window.clone());
                self.apply_layout().await?;
                self.plugin_manager.on_window_created(&window)?;
            }
            WindowEvent::WindowDestroyed(id) => {
                if let Some(window) = self.windows.remove(&id) {
                    self.apply_layout().await?;
                    self.plugin_manager.on_window_destroyed(&window)?;
                }
            }
            WindowEvent::WindowMoved(id, new_rect) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    let old_rect = window.rect;
                    window.rect = new_rect;

                    // Handle snap logic for mouse dragging
                    self.handle_window_move_snap(id, old_rect, new_rect).await?;
                }
            }
            WindowEvent::WindowResized(id, new_rect) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    window.rect = new_rect;
                }
            }
            WindowEvent::WindowFocused(id) => {
                for window in self.windows.values_mut() {
                    window.is_focused = window.id == id;
                }
                self.plugin_manager.on_window_focused(id)?;
            }
            WindowEvent::WindowMinimized(id) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    window.is_minimized = true;
                    self.apply_layout().await?;
                }
            }
            WindowEvent::WindowUnminimized(id) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    window.is_minimized = false;
                    self.apply_layout().await?;
                }
            }
            WindowEvent::WorkspaceChanged(workspace) => {
                self.current_workspace = workspace;
                self.refresh_windows().await?;
            }
            WindowEvent::MouseMoved { x, y } => {
                self.focus_manager
                    .handle_mouse_move(x, y, &self.windows)
                    .await?;
            }
        }

        Ok(())
    }

    async fn handle_command(&mut self, command: Command) -> Result<()> {
        debug!("Handling command: {:?}", command);

        match command {
            Command::FocusWindow(id) => {
                if self.windows.contains_key(&id) {
                    self.macos.focus_window(id).await?;
                }
            }
            Command::CloseWindow(id) => {
                if self.windows.contains_key(&id) {
                    self.macos.close_window(id).await?;
                }
            }
            Command::MoveWindow(id, rect) => {
                if self.windows.contains_key(&id) {
                    self.macos.move_window(id, rect).await?;
                }
            }
            Command::FocusDirection(direction) => {
                if let Some(target_id) = self.find_window_in_direction(direction) {
                    self.macos.focus_window(target_id).await?;
                    info!("Focused window in direction {:?}", direction);
                } else {
                    debug!("No window found in direction {:?}", direction);
                }
            }
            Command::MoveDirection(direction) => {
                if let Some(focused_id) = self.get_focused_window_id() {
                    if let Some(target_id) = self.find_window_in_direction(direction) {
                        // For now, just swap the focused window with the target
                        if let (Some(focused_window), Some(target_window)) =
                            (self.windows.get(&focused_id), self.windows.get(&target_id))
                        {
                            let focused_rect = focused_window.rect;
                            let target_rect = target_window.rect;

                            self.macos.move_window(focused_id, target_rect).await?;
                            self.macos.move_window(target_id, focused_rect).await?;

                            info!("Swapped windows in direction {:?}", direction);
                        }
                    }
                }
            }
            Command::CloseFocusedWindow => {
                if let Some(focused_id) = self.get_focused_window_id() {
                    self.macos.close_window(focused_id).await?;
                    info!("Closed focused window");
                }
            }
            Command::ToggleLayout => {
                self.layout_manager.toggle_layout();
                self.apply_layout().await?;
                info!(
                    "Toggled layout to: {:?}",
                    self.layout_manager.get_current_layout()
                );
            }
            Command::ToggleFloat => {
                if let Some(_focused_id) = self.get_focused_window_id() {
                    // For now, just apply layout - a full implementation would track floating state
                    self.apply_layout().await?;
                    info!("Toggled float for focused window");
                }
            }
            Command::ToggleFullscreen => {
                if let Some(focused_id) = self.get_focused_window_id() {
                    // Get screen rect and move window to fill it
                    let screen_rect = self.macos.get_screen_rect().await?;
                    self.macos.move_window(focused_id, screen_rect).await?;
                    info!("Toggled fullscreen for focused window");
                }
            }
            Command::SwapMain => {
                if let Some(focused_id) = self.get_focused_window_id() {
                    // Find the "main" window (first in layout order) and swap with focused
                    let workspace_windows: Vec<&Window> = self
                        .windows
                        .values()
                        .filter(|w| w.workspace_id == self.current_workspace && !w.is_minimized)
                        .collect();

                    if let Some(main_window) = workspace_windows.first() {
                        let main_id = main_window.id;
                        if main_id != focused_id {
                            if let (Some(focused_window), Some(main_window)) =
                                (self.windows.get(&focused_id), self.windows.get(&main_id))
                            {
                                let focused_rect = focused_window.rect;
                                let main_rect = main_window.rect;

                                self.macos.move_window(focused_id, main_rect).await?;
                                self.macos.move_window(main_id, focused_rect).await?;

                                info!("Swapped focused window with main window");
                            }
                        }
                    }
                }
            }
            Command::ReloadConfig => {
                info!("Reloading configuration");
            }
            Command::ListWindows => {
                for (id, window) in &self.windows {
                    info!("Window {}: {} ({})", id.0, window.title, window.owner);
                }
            }
            Command::GetStatus => {
                info!(
                    "Window manager status: {} windows managed",
                    self.windows.len()
                );
            }
            Command::Quit => {
                info!("Shutting down window manager");
                return Err(anyhow::anyhow!("Quit requested"));
            }
            Command::CheckSnapDragEnd(window_id, rect) => {
                self.handle_snap_drag_end(window_id, rect).await?;
            }
        }

        Ok(())
    }

    fn get_focused_window_id(&self) -> Option<WindowId> {
        self.windows.values().find(|w| w.is_focused).map(|w| w.id)
    }

    fn find_window_in_direction(&self, direction: crate::hotkeys::Direction) -> Option<WindowId> {
        let focused_id = self.get_focused_window_id()?;
        let focused_window = self.windows.get(&focused_id)?;
        let focused_center = (
            focused_window.rect.x + focused_window.rect.width / 2.0,
            focused_window.rect.y + focused_window.rect.height / 2.0,
        );

        let workspace_windows: Vec<&Window> = self
            .windows
            .values()
            .filter(|w| {
                w.workspace_id == self.current_workspace && !w.is_minimized && w.id != focused_id
            })
            .collect();

        let mut best_window: Option<WindowId> = None;
        let mut best_distance = f64::INFINITY;

        for window in workspace_windows {
            let window_center = (
                window.rect.x + window.rect.width / 2.0,
                window.rect.y + window.rect.height / 2.0,
            );

            let is_in_direction = match direction {
                crate::hotkeys::Direction::Left => window_center.0 < focused_center.0,
                crate::hotkeys::Direction::Right => window_center.0 > focused_center.0,
                crate::hotkeys::Direction::Up => window_center.1 < focused_center.1,
                crate::hotkeys::Direction::Down => window_center.1 > focused_center.1,
            };

            if is_in_direction {
                let distance = ((window_center.0 - focused_center.0).powi(2)
                    + (window_center.1 - focused_center.1).powi(2))
                .sqrt();

                if distance < best_distance {
                    best_distance = distance;
                    best_window = Some(window.id);
                }
            }
        }

        best_window
    }

    async fn refresh_windows(&mut self) -> Result<()> {
        let current_windows = self.macos.get_windows().await?;
        let old_count = self.windows.len();

        // Update current workspace
        match self.macos.get_current_workspace().await {
            Ok(workspace) => {
                if workspace != self.current_workspace {
                    debug!(
                        "Workspace changed: {} -> {}",
                        self.current_workspace, workspace
                    );
                    self.current_workspace = workspace;
                }
            }
            Err(e) => {
                warn!("Failed to get current workspace: {}", e);
            }
        }

        // Build a new window map from current windows
        let mut new_windows = HashMap::new();
        for window in current_windows {
            new_windows.insert(window.id, window);
        }

        // Replace the old window map with the new one
        self.windows = new_windows;

        let new_count = self.windows.len();
        if old_count != new_count {
            debug!(
                "Window count changed: {} -> {} windows",
                old_count, new_count
            );
            // Trigger layout update when window count changes
            self.apply_layout().await?;
        }

        Ok(())
    }

    async fn apply_layout(&mut self) -> Result<()> {
        // Use cached workspace ID
        let current_workspace = self.current_workspace;

        // Get ALL windows in the current workspace (from all applications)
        // For now, ignore workspace filtering since workspace detection is unreliable
        let workspace_windows: Vec<&Window> =
            self.windows.values().filter(|w| !w.is_minimized).collect();

        if workspace_windows.is_empty() {
            debug!("No windows to layout in workspace {}", current_workspace);
            return Ok(());
        }

        debug!(
            "Applying layout to {} windows in workspace {} from all applications using {:?}",
            workspace_windows.len(),
            current_workspace,
            self.layout_manager.get_current_layout()
        );

        for window in &workspace_windows {
            debug!(
                "  Window to layout: {} ({}) at {:?}",
                window.title, window.owner, window.rect
            );
        }

        let screen_rect = self.macos.get_screen_rect().await?;
        let layouts = self.layout_manager.compute_layout(
            &workspace_windows,
            screen_rect,
            &self.config.general,
        );

        // Use the new move_all_windows method to handle all windows at once
        let workspace_windows_vec: Vec<Window> =
            workspace_windows.iter().map(|w| (*w).clone()).collect();
        match self
            .macos
            .move_all_windows(&layouts, &workspace_windows_vec)
            .await
        {
            Ok(_) => {
                debug!("Successfully applied layout to all windows");
                // Update our internal window state
                for (window_id, rect) in layouts {
                    if let Some(window) = self.windows.get_mut(&window_id) {
                        window.rect = rect;
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Failed to apply layout to all windows: {}, falling back to individual moves",
                    e
                );

                // Fall back to individual window moves
                for (window_id, rect) in layouts {
                    debug!(
                        "Applying layout: moving window {:?} to {:?}",
                        window_id, rect
                    );
                    for attempt in 0..3 {
                        match self.macos.move_window(window_id, rect).await {
                            Ok(_) => {
                                debug!(
                                    "Successfully moved window {:?} on attempt {}",
                                    window_id,
                                    attempt + 1
                                );
                                break;
                            }
                            Err(e) if attempt < 2 => {
                                debug!(
                                    "Failed to move window {:?} on attempt {}: {}, retrying",
                                    window_id,
                                    attempt + 1,
                                    e
                                );
                                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to move window {:?} after 3 attempts: {}",
                                    window_id, e
                                );
                            }
                        }
                    }

                    // Update our internal window state
                    if let Some(window) = self.windows.get_mut(&window_id) {
                        window.rect = rect;
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_window_move_snap(
        &mut self,
        window_id: WindowId,
        old_rect: Rect,
        new_rect: Rect,
    ) -> Result<()> {
        // Calculate movement distance to detect if this is a significant move
        let dx = new_rect.x - old_rect.x;
        let dy = new_rect.y - old_rect.y;
        let movement_distance = (dx * dx + dy * dy).sqrt();

        // Only track moves that are significant and not from our own layout operations
        if movement_distance > 20.0 {
            // Higher threshold
            debug!(
                "Significant movement detected for window {:?}: {:.1}px",
                window_id, movement_distance
            );

            if !self.snap_manager.is_window_dragging(window_id) {
                debug!("Starting drag tracking for window {:?}", window_id);
                self.snap_manager.start_window_drag(window_id, old_rect);

                // Schedule a check for drag end
                let command_tx = self.command_tx.clone();
                let window_id_copy = window_id;
                let new_rect_copy = new_rect;

                tokio::spawn(async move {
                    tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
                    if let Err(e) = command_tx
                        .send(Command::CheckSnapDragEnd(window_id_copy, new_rect_copy))
                        .await
                    {
                        debug!("Failed to send snap check command: {}", e);
                    }
                });
            } else {
                self.snap_manager.update_window_drag(window_id, new_rect);
            }
        }

        Ok(())
    }

    async fn handle_snap_drag_end(
        &mut self,
        window_id: WindowId,
        current_rect: Rect,
    ) -> Result<()> {
        // Only process if window is still being dragged
        if self.snap_manager.is_window_dragging(window_id) {
            // Check if there's a snap target
            if let Some(snap_rect) = self.snap_manager.end_window_drag(window_id, current_rect) {
                // Check if the window is already very close to the snap position to avoid redundant moves
                let dx = (snap_rect.x - current_rect.x).abs();
                let dy = (snap_rect.y - current_rect.y).abs();
                let dw = (snap_rect.width - current_rect.width).abs();
                let dh = (snap_rect.height - current_rect.height).abs();

                // Only snap if there's a meaningful difference (> 5 pixels)
                if dx > 5.0 || dy > 5.0 || dw > 5.0 || dh > 5.0 {
                    debug!("Snapping window {:?} to {:?}", window_id, snap_rect);

                    // Move the window to snap position
                    self.macos.move_window(window_id, snap_rect).await?;

                    // Update our internal state
                    if let Some(window) = self.windows.get_mut(&window_id) {
                        window.rect = snap_rect;
                    }
                } else {
                    debug!(
                        "Window {:?} already close to snap position, skipping move",
                        window_id
                    );
                }
            } else {
                debug!("No snap target found for window {:?}", window_id);
            }

            // Always clear the drag state when we're done
            self.snap_manager.clear_drag_state(window_id);
        }

        Ok(())
    }

    #[allow(dead_code)]
    async fn update_layout_for_manual_move(
        &mut self,
        window_id: WindowId,
        new_rect: Rect,
    ) -> Result<()> {
        // For now, we'll just apply the existing layout logic
        // In a more sophisticated implementation, we might update the BSP tree
        // to reflect the manual positioning
        debug!(
            "Window {:?} manually moved to {:?}, updating layout",
            window_id, new_rect
        );

        // You could implement logic here to:
        // 1. Remove the window from its current position in the BSP tree
        // 2. Find where it should be placed based on its new position
        // 3. Rebuild the tree structure accordingly

        // For now, just ensure the layout is consistent
        self.apply_layout().await?;

        Ok(())
    }
}
