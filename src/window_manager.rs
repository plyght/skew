use crate::focus::FocusManager;
use crate::hotkeys::HotkeyManager;
use crate::ipc::IpcServer;
use crate::layout::LayoutManager;
use crate::macos::window_notifications::{WindowDragEvent, WindowDragNotificationObserver};
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
    #[allow(dead_code)]
    drag_observer: WindowDragNotificationObserver,
    drag_event_rx: mpsc::Receiver<WindowDragEvent>,

    // Simplified tracking: only track programmatic moves to prevent feedback
    programmatically_moving: std::collections::HashSet<WindowId>,
    
    // Track windows currently being dragged by user
    user_dragging_windows: std::collections::HashSet<WindowId>,

    // Track last known window positions for delta detection
    last_window_positions: std::collections::HashMap<WindowId, Rect>,
    
    // Simplified layout state tracking
    layout_needs_update: bool,
    last_layout_time: Option<std::time::Instant>,
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
        drag_observer
            .start_observing()
            .map_err(|e| anyhow::anyhow!("Failed to start drag observer: {}", e))?;

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
            last_window_positions: std::collections::HashMap::new(),
            layout_needs_update: true,
            last_layout_time: None,
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
        
        // Force layout application on startup
        self.layout_needs_update = true;
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
                info!("🆕 Window created: {} ({})", window.title, window.owner);
                self.windows.insert(window.id, window.clone());
                self.layout_needs_update = true;
                // Force layout application for new windows since this significantly changes the layout
                self.apply_layout_force().await?;
                self.plugin_manager.on_window_created(&window)?;
            }
            WindowEvent::WindowDestroyed(id) => {
                if let Some(window) = self.windows.remove(&id) {
                    info!("🗑️ Window destroyed: {} ({})", window.title, window.owner);
                    self.layout_needs_update = true;
                    // Force layout application for destroyed windows since this significantly changes the layout
                    self.apply_layout_force().await?;
                    self.plugin_manager.on_window_destroyed(&window)?;
                }
            }
            WindowEvent::WindowMoved(id, new_rect) => {
                debug!("🔄 Window {:?} moved to {:?}", id, new_rect);
                
                // Check if this is a programmatic move
                let is_programmatic = self.programmatically_moving.contains(&id);
                let is_user_dragging = self.user_dragging_windows.contains(&id);
                
                debug!("  -> Programmatic: {}, User dragging: {}", is_programmatic, is_user_dragging);
                
                // Handle programmatic move cleanup - just update position and clear flag
                if is_programmatic {
                    debug!("✅ Programmatic move completed for window {:?}", id);
                    self.programmatically_moving.remove(&id);
                    if let Some(window) = self.windows.get_mut(&id) {
                        window.rect = new_rect;
                    }
                    self.last_window_positions.insert(id, new_rect);
                    return Ok(());
                }
                
                // Update window position
                if let Some(window) = self.windows.get_mut(&id) {
                    window.rect = new_rect;
                }
                
                // If user is dragging, let the drag system handle positioning
                if is_user_dragging {
                    debug!("🖱️ Window {:?} moved during user drag", id);
                    self.last_window_positions.insert(id, new_rect);
                    return Ok(());
                }
                
                // This is a manual user move - trigger positioning logic
                debug!("👤 Manual user move detected for window {:?}", id);
                self.handle_user_window_move(id, new_rect).await?;
                
                // Update position tracking AFTER processing the move
                self.last_window_positions.insert(id, new_rect);
            }
            WindowEvent::WindowResized(id, new_rect) => {
                debug!("📏 Window {:?} resized to {:?}", id, new_rect);
                
                // Update window position
                if let Some(window) = self.windows.get_mut(&id) {
                    window.rect = new_rect;
                }
                
                // For manual resizes, snap back to proper layout size
                debug!("👤 Manual resize detected - snapping back to layout");
                self.handle_window_resize(id, new_rect).await?;
                
                // Update position tracking AFTER processing the resize
                self.last_window_positions.insert(id, new_rect);
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
                    self.layout_needs_update = true;
                    // Don't apply layout immediately - let it happen naturally
                }
            }
            WindowEvent::WindowUnminimized(id) => {
                if let Some(window) = self.windows.get_mut(&id) {
                    window.is_minimized = false;
                    self.layout_needs_update = true;
                    // Apply layout when window comes back since it should be positioned
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
                self.layout_needs_update = true;
                self.apply_layout().await?;
                info!(
                    "Toggled layout to: {:?}",
                    self.layout_manager.get_current_layout()
                );
            }
            Command::ToggleFloat => {
                if let Some(_focused_id) = self.get_focused_window_id() {
                    // For now, just apply layout - a full implementation would track floating state
                    self.layout_needs_update = true;
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
            WindowDragEvent::DragStarted {
                window_id,
                initial_rect,
                owner_pid,
            } => {
                info!(
                    "🚀 DRAG STARTED (NSWindow): window {:?} at {:?} (PID: {})",
                    window_id, initial_rect, owner_pid
                );

                // Track that this window is being dragged by the user
                self.user_dragging_windows.insert(window_id);

                // Start tracking this drag in the snap manager
                self.snap_manager.start_window_drag(window_id, initial_rect);

                // Store the original position for potential restoration
                self.last_window_positions
                    .insert(window_id, initial_rect);
            }
            WindowDragEvent::DragEnded {
                window_id,
                final_rect,
                owner_pid,
            } => {
                info!(
                    "🛑 DRAG ENDED (NSWindow): window {:?} at {:?} (PID: {})",
                    window_id, final_rect, owner_pid
                );

                // Remove from user dragging set first
                self.user_dragging_windows.remove(&window_id);

                // Check if this window is managed by us
                if self.windows.contains_key(&window_id) {
                    // Update our internal state with final position
                    if let Some(window) = self.windows.get_mut(&window_id) {
                        window.rect = final_rect;
                    }

                    // Get the initial rect from snap manager for drag processing
                    if self.snap_manager.is_window_dragging(window_id) {
                        // Get current windows for accurate workspace filtering
                        let current_windows = self.macos.get_windows().await?;
                        let effective_workspace = self.get_effective_current_workspace();
                        let workspace_windows: Vec<&crate::Window> = current_windows
                            .iter()
                            .filter(|w| w.workspace_id == effective_workspace && !w.is_minimized)
                            .collect();

                        // Process the drag end with snap manager
                        let drag_result = self.snap_manager.end_window_drag(
                            window_id,
                            final_rect,
                            &workspace_windows,
                        );

                        match drag_result {
                            crate::snap::DragResult::SnapToZone(snap_rect) => {
                                info!(
                                    "📍 Snapping dragged window {:?} to zone at {:?}",
                                    window_id, snap_rect
                                );
                                self.programmatically_moving.insert(window_id);
                                if let Err(e) = self.macos.move_window(window_id, snap_rect).await {
                                    warn!("❌ Failed to snap window after drag: {}", e);
                                } else {
                                    if let Some(window) = self.windows.get_mut(&window_id) {
                                        window.rect = snap_rect;
                                    }
                                    self.last_window_positions.insert(window_id, snap_rect);
                                }
                            }
                            crate::snap::DragResult::SwapWithWindow(target_id, original_rect) => {
                                info!(
                                    "🔄 Swapping dragged window {:?} with target {:?}",
                                    window_id, target_id
                                );
                                // Use the enhanced swap_windows method
                                if let Err(e) = self
                                    .swap_windows_with_rects(window_id, target_id, original_rect)
                                    .await
                                {
                                    warn!("❌ Failed to swap windows after drag: {}", e);
                                }
                            }
                            crate::snap::DragResult::ReturnToOriginal(original_rect) => {
                                info!(
                                    "↩️ Returning dragged window {:?} to original position {:?}",
                                    window_id, original_rect
                                );
                                self.programmatically_moving.insert(window_id);
                                if let Err(e) =
                                    self.macos.move_window(window_id, original_rect).await
                                {
                                    warn!("❌ Failed to return window to original position: {}", e);
                                } else {
                                    if let Some(window) = self.windows.get_mut(&window_id) {
                                        window.rect = original_rect;
                                    }
                                    self.last_window_positions
                                        .insert(window_id, original_rect);
                                }
                            }
                            crate::snap::DragResult::NoAction => {
                                debug!("No action needed for dragged window {:?}", window_id);
                            }
                        }

                        // Clear drag state
                        self.snap_manager.clear_drag_state(window_id);
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_user_window_move(
        &mut self,
        window_id: WindowId,
        new_rect: Rect,
    ) -> Result<()> {
        debug!("🎯 handle_user_window_move called for window {:?} at {:?}", window_id, new_rect);
        
        // Get previous position to check for significant movement
        let prev_rect = self.last_window_positions.get(&window_id).copied();
        debug!("  -> Previous position: {:?}", prev_rect);

        if let Some(prev_rect) = prev_rect {
            let dx = (new_rect.x - prev_rect.x).abs();
            let dy = (new_rect.y - prev_rect.y).abs();
            let distance = (dx * dx + dy * dy).sqrt();
            
            debug!("  -> Movement distance: {:.1}px (dx: {:.1}, dy: {:.1})", distance, dx, dy);
            
            // Very low threshold to catch even small movements (>0.5px)
            if distance > 0.5 {
                debug!("🔄 User move detected for window {:?}: {:.1}px from {:?} to {:?}", 
                       window_id, distance, prev_rect, new_rect);

                // For manual moves, snap back to proper layout position instead of snap zones
                let layout_target = self.get_layout_target_for_window(window_id).await;
                
                if let Some(target_rect) = layout_target {
                    let layout_distance = ((target_rect.x - new_rect.x).powi(2) + (target_rect.y - new_rect.y).powi(2)).sqrt();
                    
                    debug!("🎯 Found layout target for window {:?}: {:?} (distance: {:.1}px)", 
                           window_id, target_rect, layout_distance);
                    
                    // If window is significantly displaced from its layout position, snap it back
                    if layout_distance > 10.0 {
                        info!("📌 Snapping window {:?} back to layout at {:?}", window_id, target_rect);
                        self.programmatically_moving.insert(window_id);
                        
                        if let Err(e) = self.macos.move_window(window_id, target_rect).await {
                            warn!("❌ Failed to snap window back to layout: {}", e);
                        } else {
                            // Update our internal state
                            if let Some(window) = self.windows.get_mut(&window_id) {
                                window.rect = target_rect;
                            }
                            debug!("✅ Successfully snapped window {:?} back to layout", window_id);
                        }
                        return Ok(());
                    } else {
                        debug!("📍 Window already close to layout position: {:.1}px", layout_distance);
                    }
                } else {
                    debug!("🚫 No layout target found for window {:?}", window_id);
                }
                
                // If no snapping occurred, just note the move - don't reapply layout immediately
                // This prevents feedback loops where layout application triggers more moves
                debug!("Manual window move detected - layout may be disrupted");
            } else {
                debug!("⚠️ Movement too small ({:.1}px) - ignoring", distance);
            }
        } else {
            debug!("📍 First position recorded for window {:?} - no snapping available yet", window_id);
        }

        Ok(())
    }

    async fn get_layout_target_for_window(&self, window_id: WindowId) -> Option<Rect> {
        // Get the computed layout for this window
        let effective_workspace = self.get_effective_current_workspace();
        let workspace_windows: Vec<Window> = self
            .windows
            .values()
            .filter(|w| w.workspace_id == effective_workspace && !w.is_minimized)
            .cloned()
            .collect();

        if workspace_windows.is_empty() {
            return None;
        }

        // We need a mutable reference to layout_manager, but we're in an immutable method
        // Let's compute the layout directly using the same logic
        let screen_rect = match self.macos.get_screen_rect().await {
            Ok(rect) => rect,
            Err(_) => return None,
        };
        
        let workspace_refs: Vec<&Window> = workspace_windows.iter().collect();
        
        // Create a temporary copy of the layout manager to compute layout
        let mut temp_layout_manager = self.layout_manager.clone();
        let layouts = temp_layout_manager.compute_layout(
            &workspace_refs,
            screen_rect,
            &self.config.general,
        );

        layouts.get(&window_id).copied()
    }

    async fn handle_window_resize(
        &mut self,
        window_id: WindowId,
        current_rect: Rect,
    ) -> Result<()> {
        debug!("🔧 handle_window_resize called for window {:?} at {:?}", window_id, current_rect);
        
        // Get the computed layout for this window to see what size it should be
        let effective_workspace = self.get_effective_current_workspace();
        let workspace_windows: Vec<Window> = self
            .windows
            .values()
            .filter(|w| w.workspace_id == effective_workspace && !w.is_minimized)
            .cloned()
            .collect();

        if workspace_windows.is_empty() {
            debug!("No windows to layout - skipping resize handling");
            return Ok(());
        }

        let screen_rect = self.macos.get_screen_rect().await?;
        let workspace_refs: Vec<&Window> = workspace_windows.iter().collect();
        let layouts = self.layout_manager.compute_layout(
            &workspace_refs,
            screen_rect,
            &self.config.general,
        );

        if let Some(target_rect) = layouts.get(&window_id) {
            let pos_distance = ((target_rect.x - current_rect.x).powi(2) + (target_rect.y - current_rect.y).powi(2)).sqrt();
            let size_distance = ((target_rect.width - current_rect.width).powi(2) + (target_rect.height - current_rect.height).powi(2)).sqrt();
            
            debug!("  -> Target layout: {:?}", target_rect);
            debug!("  -> Position distance: {:.1}px, Size distance: {:.1}px", pos_distance, size_distance);
            
            // If window is significantly different from layout, snap it back
            if pos_distance > 10.0 || size_distance > 10.0 {
                info!("📐 Snapping resized window {:?} back to layout at {:?}", window_id, target_rect);
                self.programmatically_moving.insert(window_id);
                
                if let Err(e) = self.macos.move_window(window_id, *target_rect).await {
                    warn!("❌ Failed to snap resized window back to layout: {}", e);
                } else {
                    // Update our internal state
                    if let Some(window) = self.windows.get_mut(&window_id) {
                        window.rect = *target_rect;
                    }
                    debug!("✅ Successfully snapped resized window {:?} back to layout", window_id);
                }
            } else {
                debug!("📐 Window already close to target layout - no snapping needed");
            }
        } else {
            debug!("⚠️ No layout target found for window {:?}", window_id);
        }

        Ok(())
    }

    async fn swap_windows(&mut self, window1_id: WindowId, window2_id: WindowId) -> Result<()> {
        // Get the most current window positions, not cached ones
        let current_windows = self.macos.get_windows().await?;

        let window1_current = current_windows.iter().find(|w| w.id == window1_id);
        let window2_current = current_windows.iter().find(|w| w.id == window2_id);

        if let (Some(window1), Some(window2)) = (window1_current, window2_current) {
            let window1_rect = window1.rect;
            let window2_rect = window2.rect;

            debug!(
                "🔄 Swapping positions of windows {:?} (at {:?}) and {:?} (at {:?})",
                window1_id, window1_rect, window2_id, window2_rect
            );

            // Mark both as programmatic moves to avoid feedback loops
            self.programmatically_moving.insert(window1_id);
            self.programmatically_moving.insert(window2_id);

            // Create swap layout
            let mut swap_layouts = HashMap::new();
            swap_layouts.insert(window1_id, window2_rect);
            swap_layouts.insert(window2_id, window1_rect);

            // Try bulk move first (more reliable)
            let both_windows = vec![window1.clone(), window2.clone()];
            match self
                .macos
                .move_all_windows(&swap_layouts, &both_windows)
                .await
            {
                Ok(_) => {
                    debug!("✅ Successfully swapped windows using bulk move");
                    // Update our internal state
                    if let Some(w) = self.windows.get_mut(&window1_id) {
                        w.rect = window2_rect;
                    }
                    if let Some(w) = self.windows.get_mut(&window2_id) {
                        w.rect = window1_rect;
                    }
                    self.last_window_positions
                        .insert(window1_id, window2_rect);
                    self.last_window_positions
                        .insert(window2_id, window1_rect);
                }
                Err(e) => {
                    warn!("Bulk swap failed, trying individual moves: {}", e);

                    // Fallback to individual moves
                    match self.macos.move_window(window1_id, window2_rect).await {
                        Ok(_) => {
                            if let Some(w) = self.windows.get_mut(&window1_id) {
                                w.rect = window2_rect;
                            }
                            self.last_window_positions
                                .insert(window1_id, window2_rect);
                        }
                        Err(e) => {
                            warn!("Failed to move window {:?} during swap: {}", window1_id, e)
                        }
                    }

                    match self.macos.move_window(window2_id, window1_rect).await {
                        Ok(_) => {
                            if let Some(w) = self.windows.get_mut(&window2_id) {
                                w.rect = window1_rect;
                            }
                            self.last_window_positions
                                .insert(window2_id, window1_rect);
                        }
                        Err(e) => {
                            warn!("Failed to move window {:?} during swap: {}", window2_id, e)
                        }
                    }
                }
            }
        } else {
            warn!(
                "Could not find current positions for windows {:?} and {:?}",
                window1_id, window2_id
            );
        }
        Ok(())
    }

    async fn swap_windows_with_rects(
        &mut self,
        window1_id: WindowId,
        window2_id: WindowId,
        window1_original_rect: Rect,
    ) -> Result<()> {
        // Get current window positions for the target window
        let current_windows = self.macos.get_windows().await?;
        let window2_current = current_windows.iter().find(|w| w.id == window2_id);

        if let Some(window2) = window2_current {
            let window2_rect = window2.rect;

            debug!(
                "🔄 Swapping positions: window {:?} to {:?}, window {:?} to {:?}",
                window1_id, window2_rect, window2_id, window1_original_rect
            );

            // Mark both as programmatic moves to avoid feedback loops
            self.programmatically_moving.insert(window1_id);
            self.programmatically_moving.insert(window2_id);

            // Create swap layout
            let mut swap_layouts = HashMap::new();
            swap_layouts.insert(window1_id, window2_rect);
            swap_layouts.insert(window2_id, window1_original_rect);

            // Get the current window object for window1
            let window1_current = current_windows.iter().find(|w| w.id == window1_id);

            if let Some(window1) = window1_current {
                let both_windows = vec![window1.clone(), window2.clone()];

                // Try bulk move first (more reliable)
                match self
                    .macos
                    .move_all_windows(&swap_layouts, &both_windows)
                    .await
                {
                    Ok(_) => {
                        debug!("✅ Successfully swapped windows using bulk move");
                        // Update our internal state
                        if let Some(w) = self.windows.get_mut(&window1_id) {
                            w.rect = window2_rect;
                        }
                        if let Some(w) = self.windows.get_mut(&window2_id) {
                            w.rect = window1_original_rect;
                        }
                        self.last_window_positions
                            .insert(window1_id, window2_rect);
                        self.last_window_positions
                            .insert(window2_id, window1_original_rect);
                    }
                    Err(e) => {
                        warn!("Bulk swap failed, trying individual moves: {}", e);

                        // Fallback to individual moves
                        match self.macos.move_window(window1_id, window2_rect).await {
                            Ok(_) => {
                                if let Some(w) = self.windows.get_mut(&window1_id) {
                                    w.rect = window2_rect;
                                }
                                self.last_window_positions
                                    .insert(window1_id, window2_rect);
                            }
                            Err(e) => {
                                warn!("Failed to move window {:?} during swap: {}", window1_id, e)
                            }
                        }

                        match self
                            .macos
                            .move_window(window2_id, window1_original_rect)
                            .await
                        {
                            Ok(_) => {
                                if let Some(w) = self.windows.get_mut(&window2_id) {
                                    w.rect = window1_original_rect;
                                }
                                self.last_window_positions
                                    .insert(window2_id, window1_original_rect);
                            }
                            Err(e) => {
                                warn!("Failed to move window {:?} during swap: {}", window2_id, e)
                            }
                        }
                    }
                }
            } else {
                warn!("Could not find current window {:?} for swap", window1_id);
            }
        } else {
            warn!("Could not find target window {:?} for swap", window2_id);
        }
        Ok(())
    }

    async fn return_window_to_original(
        &mut self,
        window_id: WindowId,
        original_rect: Rect,
    ) -> Result<()> {
        debug!(
            "↩️ Returning window {:?} to original position {:?}",
            window_id, original_rect
        );

        // Mark as programmatic move
        self.programmatically_moving.insert(window_id);

        // Move the window back
        match self.macos.move_window(window_id, original_rect).await {
            Ok(_) => {
                if let Some(window) = self.windows.get_mut(&window_id) {
                    window.rect = original_rect;
                }
                self.last_window_positions
                    .insert(window_id, original_rect);
            }
            Err(e) => warn!(
                "Failed to return window {:?} to original position: {}",
                window_id, e
            ),
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
            if let std::collections::hash_map::Entry::Vacant(e) = self.last_window_positions.entry(window.id) {
                e.insert(window.rect);
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
            // Mark layout as needing update, but don't apply immediately
            // This prevents rapid layout applications during window enumeration
            self.layout_needs_update = true;
        }

        Ok(())
    }

    async fn apply_layout(&mut self) -> Result<()> {
        self.apply_layout_internal(false).await
    }

    async fn apply_layout_force(&mut self) -> Result<()> {
        self.apply_layout_internal(true).await
    }

    async fn apply_layout_internal(&mut self, force: bool) -> Result<()> {
        // Throttle layout applications to prevent rapid fire updates, unless forced
        if !force {
            let now = std::time::Instant::now();
            if let Some(last_time) = self.last_layout_time {
                if now.duration_since(last_time).as_millis() < 200 {
                    debug!("Layout application throttled - too soon since last application");
                    return Ok(());
                }
            }
            self.last_layout_time = Some(now);
        } else {
            self.last_layout_time = Some(std::time::Instant::now());
            debug!("🚀 Forcing layout application");
        }
        
        // Use effective workspace detection for more reliable filtering
        let effective_workspace = self.get_effective_current_workspace();

        // Get windows in the effective current workspace - collect to owned to avoid borrow issues
        let workspace_windows: Vec<Window> = self
            .windows
            .values()
            .filter(|w| w.workspace_id == effective_workspace && !w.is_minimized)
            .cloned()
            .collect();

        if workspace_windows.is_empty() {
            debug!("No windows to layout in workspace {}", effective_workspace);
            return Ok(());
        }
        
        debug!("=== LAYOUT APPLICATION START ===");
        debug!("Found {} windows to layout in workspace {}", workspace_windows.len(), effective_workspace);


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
        let workspace_refs: Vec<&Window> = workspace_windows.iter().collect();
        let layouts = self.layout_manager.compute_layout(
            &workspace_refs,
            screen_rect,
            &self.config.general,
        );

        debug!("Layout computation complete: {} positions calculated", layouts.len());
        for (window_id, rect) in &layouts {
            debug!("  -> Window {:?} should be at {:?}", window_id, rect);
        }
        
        // Mark all windows as being moved programmatically
        for window_id in layouts.keys() {
            self.programmatically_moving.insert(*window_id);
        }

        // Use the new move_all_windows method to handle all windows at once
        let workspace_windows_vec: Vec<Window> = workspace_windows.clone();
        match self
            .macos
            .move_all_windows(&layouts, &workspace_windows_vec)
            .await
        {
            Ok(_) => {
                debug!("Successfully applied layout to all windows");
                // Update our internal window state AND position tracking
                for (window_id, rect) in layouts {
                    if let Some(window) = self.windows.get_mut(&window_id) {
                        window.rect = rect;
                    }
                    // Update position tracking to prevent feedback loops
                    self.last_window_positions.insert(window_id, rect);
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

                    // Update our internal window state and position tracking
                    if let Some(window) = self.windows.get_mut(&window_id) {
                        window.rect = rect;
                    }
                    // Update position tracking to prevent feedback loops
                    self.last_window_positions.insert(window_id, rect);
                }
            }
        }
        
        debug!("=== LAYOUT APPLICATION END ===");
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
