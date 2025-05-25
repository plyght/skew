use crate::focus::FocusManager;
use crate::hotkeys::HotkeyManager;
use crate::ipc::IpcServer;
use crate::layout::LayoutManager;
use crate::macos::MacOSWindowSystem;
use crate::plugins::PluginManager;
use crate::{Config, Rect, Result, WindowId};
use log::{debug, error, info};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

#[derive(Debug, Clone)]
pub struct Window {
    pub id: WindowId,
    pub title: String,
    pub owner: String,
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
    CloseWindow(WindowId),
    MoveWindow(WindowId, Rect),
    ToggleLayout,
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

    event_rx: mpsc::Receiver<WindowEvent>,
    command_rx: mpsc::Receiver<Command>,
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
            event_rx,
            command_rx,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        info!("Starting window manager event loop");

        self.macos.start_monitoring().await?;
        self.focus_manager.start().await?;
        self.ipc_server.start().await?;
        self.hotkey_manager.start().await?;

        let mut refresh_timer = interval(Duration::from_millis(100));

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
                    window.rect = new_rect;
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
            Command::ToggleLayout => {
                self.layout_manager.toggle_layout();
                self.apply_layout().await?;
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

    async fn refresh_windows(&mut self) -> Result<()> {
        let current_windows = self.macos.get_windows().await?;

        for window in current_windows {
            self.windows.insert(window.id, window);
        }

        Ok(())
    }

    async fn apply_layout(&mut self) -> Result<()> {
        let workspace_windows: Vec<&Window> = self
            .windows
            .values()
            .filter(|w| w.workspace_id == self.current_workspace && !w.is_minimized)
            .collect();

        if workspace_windows.is_empty() {
            return Ok(());
        }

        let screen_rect = self.macos.get_screen_rect().await?;
        let layouts = self.layout_manager.compute_layout(
            &workspace_windows,
            screen_rect,
            &self.config.general,
        );

        for (window_id, rect) in layouts {
            self.macos.move_window(window_id, rect).await?;
        }

        Ok(())
    }
}
