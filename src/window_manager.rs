use crate::focus::FocusManager;
use crate::hotkeys::HotkeyManager;
use crate::ipc::IpcServer;
use crate::layout::LayoutManager;
use crate::macos::window_notifications::{WindowDragEvent, WindowDragNotificationObserver};
use crate::macos::MacOSWindowSystem;
use crate::plugins::PluginManager;
use crate::snap::{DragResult, SnapManager};
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
    #[allow(dead_code)]
    command_tx: mpsc::Sender<Command>,
    
    // Drag notification system
    drag_observer: WindowDragNotificationObserver,
    drag_event_rx: mpsc::Receiver<WindowDragEvent>,

    // Track windows being moved programmatically to avoid snap conflicts
    programmatically_moving: std::collections::HashSet<WindowId>,

    // Track actual user drag state (via NSWindow notifications)
    user_dragging_windows: std::collections::HashSet<WindowId>,

    // Track window previous positions for immediate drag detection
    previous_window_positions: std::collections::HashMap<WindowId, Rect>,
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

        // Set up drag notification system using NSWindow notifications
        let (drag_event_tx, drag_event_rx) = mpsc::channel(100);
        let mut drag_observer = WindowDragNotificationObserver::new(drag_event_tx);
        drag_observer.start_observing().map_err(|e| anyhow::anyhow!("Failed to start drag observer: {}", e))?;

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
            drag_observer,
            drag_event_rx,
            programmatically_moving: std::collections::HashSet::new(),
            user_dragging_windows: std::collections::HashSet::new(),
            previous_window_positions: std::collections::HashMap::new(),
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
                Some(drag_event) = self.drag_event_rx.recv() => {
                    if let Err(e) = self.handle_drag_event(drag_event).await {
                        error!("Error handling drag event: {}", e);
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
                // Handle programmatic move cleanup
                if self.programmatically_moving.contains(&id) {
                    debug!("Ignoring programmatic move for window {:?}", id);
                    self.programmatically_moving.remove(&id);
                    if let Some(window) = self.windows.get_mut(&id) {
                        window.rect = new_rect;
                    }
                } else {
                    // Update window position - drag detection now handled by NSWindow notifications
                    debug!("Window {:?} moved to {:?}", id, new_rect);
                    if let Some(window) = self.windows.get_mut(&id) {
                        window.rect = new_rect;
                    }
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
                    self.programmatically_moving.insert(id);
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

                            self.programmatically_moving.insert(focused_id);
                            self.programmatically_moving.insert(target_id);
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
                    self.programmatically_moving.insert(focused_id);
                    self.macos.move_window(focused_id, screen_rect).await?;
                    info!("Toggled fullscreen for focused window");
                }
            }
            Command::SwapMain => {
                if let Some(focused_id) = self.get_focused_window_id() {
                    // Find the "main" window (first in layout order) and swap with focused
                    let effective_workspace = self.get_effective_current_workspace();
                    let workspace_windows: Vec<&Window> = self
                        .windows
                        .values()
                        .filter(|w| w.workspace_id == effective_workspace && !w.is_minimized)
                        .collect();

                    if let Some(main_window) = workspace_windows.first() {
                        let main_id = main_window.id;
                        if main_id != focused_id {
                            if let (Some(focused_window), Some(main_window)) =
                                (self.windows.get(&focused_id), self.windows.get(&main_id))
                            {
                                let focused_rect = focused_window.rect;
                                let main_rect = main_window.rect;

                                self.programmatically_moving.insert(focused_id);
                                self.programmatically_moving.insert(main_id);
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
        }

        Ok(())
    }

    async fn handle_drag_event(&mut self, event: WindowDragEvent) -> Result<()> {
        match event {
            WindowDragEvent::DragStarted { window_id, initial_rect, owner_pid } => {
                info!("🚀 DRAG STARTED (NSWindow): window {:?} at {:?} (PID: {})", window_id, initial_rect, owner_pid);
                
                // Track that this window is being dragged by the user
                self.user_dragging_windows.insert(window_id);
                
                // Start tracking this drag in the snap manager
                self.snap_manager.start_window_drag(window_id, initial_rect);
            }
            WindowDragEvent::DragEnded { window_id, final_rect, owner_pid } => {
                info!("🛑 DRAG ENDED (NSWindow): window {:?} at {:?} (PID: {})", window_id, final_rect, owner_pid);
                
                // Remove from user dragging set
                self.user_dragging_windows.remove(&window_id);
                
                // Check if this window is managed by us
                if self.windows.contains_key(&window_id) {
                    // Get the initial rect from snap manager
                    if self.snap_manager.is_window_dragging(window_id) {
                        // Process the drag end with snap manager
                        self.handle_drag_end(window_id, final_rect, final_rect).await?;
                    }
                }
            }
        }
        Ok(())
    }

    fn get_focused_window_id(&self) -> Option<WindowId> {
        self.windows.values().find(|w| w.is_focused).map(|w| w.id)
    }

    fn get_effective_current_workspace(&self) -> u32 {
        // Try to get workspace from focused window for more reliable detection
        if let Some(focused_window) = self.windows.values().find(|w| w.is_focused) {
            debug!(
                "Using focused window's workspace {} for effective workspace detection",
                focused_window.workspace_id
            );
            return focused_window.workspace_id;
        }

        // If no focused window, use the most common workspace among visible windows
        let mut workspace_counts: std::collections::HashMap<u32, usize> =
            std::collections::HashMap::new();
        for window in self.windows.values().filter(|w| !w.is_minimized) {
            *workspace_counts.entry(window.workspace_id).or_insert(0) += 1;
        }

        if let Some((&most_common_workspace, _)) =
            workspace_counts.iter().max_by_key(|(_, &count)| count)
        {
            debug!(
                "Using most common workspace {} for effective workspace detection",
                most_common_workspace
            );
            return most_common_workspace;
        }

        // Final fallback to stored current_workspace
        debug!(
            "Falling back to stored current_workspace {} for effective workspace detection",
            self.current_workspace
        );
        self.current_workspace
    }

    fn find_window_in_direction(&self, direction: crate::hotkeys::Direction) -> Option<WindowId> {
        let focused_id = self.get_focused_window_id()?;
        let focused_window = self.windows.get(&focused_id)?;
        let focused_center = (
            focused_window.rect.x + focused_window.rect.width / 2.0,
            focused_window.rect.y + focused_window.rect.height / 2.0,
        );

        let effective_workspace = self.get_effective_current_workspace();
        let workspace_windows: Vec<&Window> = self
            .windows
            .values()
            .filter(|w| {
                w.workspace_id == effective_workspace && !w.is_minimized && w.id != focused_id
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
            // Store initial positions for new windows
            if !self.previous_window_positions.contains_key(&window.id) {
                self.previous_window_positions
                    .insert(window.id, window.rect);
            }
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
        // Use effective workspace detection for more reliable filtering
        let effective_workspace = self.get_effective_current_workspace();

        // Get windows in the effective current workspace
        let workspace_windows: Vec<&Window> = self
            .windows
            .values()
            .filter(|w| w.workspace_id == effective_workspace && !w.is_minimized)
            .collect();

        if workspace_windows.is_empty() {
            debug!("No windows to layout in workspace {}", effective_workspace);
            return Ok(());
        }

        debug!(
            "Applying layout to {} windows in workspace {} using {:?}",
            workspace_windows.len(),
            effective_workspace,
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

        // Mark all windows as being moved programmatically
        for window_id in layouts.keys() {
            self.programmatically_moving.insert(*window_id);
        }

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


    async fn handle_drag_end(
        &mut self,
        window_id: WindowId,
        _initial_rect: Rect,
        final_rect: Rect,
    ) -> Result<()> {
        // Get current workspace from focused window for reliable workspace detection
        let effective_workspace = self.get_effective_current_workspace();

        // Get all windows for collision detection (filter by effective workspace)
        let workspace_windows: Vec<&Window> = self
            .windows
            .values()
            .filter(|w| w.workspace_id == effective_workspace && !w.is_minimized)
            .collect();

        info!(
            "🔍 Found {} windows in workspace for collision detection",
            workspace_windows.len()
        );

        // Check what should happen with this drag
        let drag_result =
            self.snap_manager
                .end_window_drag(window_id, final_rect, &workspace_windows);

        info!("🎯 Drag result: {:?}", drag_result);

        match drag_result {
            DragResult::SnapToZone(snap_rect) => {
                // Check if window needs to be moved (avoid redundant moves)
                let dx = (snap_rect.x - final_rect.x).abs();
                let dy = (snap_rect.y - final_rect.y).abs();
                let dw = (snap_rect.width - final_rect.width).abs();
                let dh = (snap_rect.height - final_rect.height).abs();

                if dx > 5.0 || dy > 5.0 || dw > 5.0 || dh > 5.0 {
                    info!(
                        "📍 Snapping window {:?} to zone at {:?}",
                        window_id, snap_rect
                    );

                    // Mark as programmatic move
                    self.programmatically_moving.insert(window_id);
                    
                    // Move the window to snap position (no delay needed with proper notifications)
                    match self.macos.move_window(window_id, snap_rect).await {
                        Ok(_) => info!("✅ Successfully snapped window {:?} to zone", window_id),
                        Err(e) => warn!("❌ Failed to snap window {:?}: {}", window_id, e),
                    }

                    // Update our internal state
                    if let Some(window) = self.windows.get_mut(&window_id) {
                        window.rect = snap_rect;
                    }
                }
            }
            DragResult::SwapWithWindow(target_window_id, original_rect) => {
                info!(
                    "🔄 Swapping window {:?} with window {:?}",
                    window_id, target_window_id
                );

                // Get the target window's rect
                if let Some(target_window) = self.windows.get(&target_window_id) {
                    let target_rect = target_window.rect;

                    // Mark both windows as programmatic moves
                    self.programmatically_moving.insert(window_id);
                    self.programmatically_moving.insert(target_window_id);

                    // Create layouts for both windows in their swapped positions
                    let mut swap_layouts = std::collections::HashMap::new();
                    swap_layouts.insert(window_id, target_rect);
                    swap_layouts.insert(target_window_id, original_rect);

                    // Get both window objects for the bulk move API
                    let both_windows: Vec<crate::Window> = [window_id, target_window_id]
                        .iter()
                        .filter_map(|id| self.windows.get(id).cloned())
                        .collect();

                    info!("🔄 Executing swap: dragged window {:?} -> {:?}, target window {:?} -> {:?}", 
                          window_id, target_rect, target_window_id, original_rect);

                    // Use the bulk move API which tends to be more reliable
                    match self.macos.move_all_windows(&swap_layouts, &both_windows).await {
                        Ok(_) => {
                            // Update our internal state
                            if let Some(window) = self.windows.get_mut(&window_id) {
                                window.rect = target_rect;
                            }
                            if let Some(window) = self.windows.get_mut(&target_window_id) {
                                window.rect = original_rect;
                            }

                            info!(
                                "✅ Successfully swapped windows {:?} and {:?}",
                                window_id, target_window_id
                            );
                        }
                        Err(e) => {
                            warn!("Bulk swap failed, trying individual moves: {}", e);
                            
                            // Fallback to individual moves
                            match self.macos.move_window(window_id, target_rect).await {
                                Ok(_) => info!("✅ Moved dragged window to target position"),
                                Err(e) => warn!("❌ Failed to move dragged window: {}", e),
                            }
                            
                            match self.macos.move_window(target_window_id, original_rect).await {
                                Ok(_) => info!("✅ Moved target window to original position"),
                                Err(e) => warn!("❌ Failed to move target window: {}", e),
                            }

                            // Update our internal state
                            if let Some(window) = self.windows.get_mut(&window_id) {
                                window.rect = target_rect;
                            }
                            if let Some(window) = self.windows.get_mut(&target_window_id) {
                                window.rect = original_rect;
                            }

                            info!(
                                "✅ Completed swap with individual moves: {:?} and {:?}",
                                window_id, target_window_id
                            );
                        }
                    }
                }
            }
            DragResult::ReturnToOriginal(original_rect) => {
                info!(
                    "↩️ Returning window {:?} to original position {:?}",
                    window_id, original_rect
                );

                // Mark as programmatic move
                self.programmatically_moving.insert(window_id);
                
                // Move the window back to its original position
                match self.macos.move_window(window_id, original_rect).await {
                    Ok(_) => info!("✅ Successfully returned window {:?} to original position", window_id),
                    Err(e) => warn!("❌ Failed to return window {:?} to original position: {}", window_id, e),
                }

                // Update our internal state
                if let Some(window) = self.windows.get_mut(&window_id) {
                    window.rect = original_rect;
                }
            }
            DragResult::NoAction => {
                info!("❌ No action needed for window {:?}", window_id);
            }
        }

        // Always clear the drag state when we're done
        self.snap_manager.clear_drag_state(window_id);
        info!("🧹 Cleared drag state for window {:?}", window_id);

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
