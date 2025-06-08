use crate::config::PluginConfig;
use crate::{Result, Window, WindowId};
use libloading::{Library, Symbol};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[cfg(feature = "scripting")]
use mlua::Lua;

pub trait Plugin {
    fn name(&self) -> &str;
    fn init(&mut self) -> Result<()>;
    fn on_window_created(&mut self, window: &Window) -> Result<()>;
    fn on_window_destroyed(&mut self, window: &Window) -> Result<()>;
    fn on_window_focused(&mut self, window_id: WindowId) -> Result<()>;
    fn shutdown(&mut self) -> Result<()>;
}

pub struct PluginManager {
    config: PluginConfig,
    native_plugins: HashMap<String, Box<dyn Plugin>>,
    native_libraries: HashMap<String, Library>,

    #[cfg(feature = "scripting")]
    lua_plugins: HashMap<String, LuaPlugin>,

    #[cfg(feature = "scripting")]
    lua: Lua,
}

#[cfg(feature = "scripting")]
pub struct LuaPlugin {
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    script_path: PathBuf,
}

impl PluginManager {
    pub fn new(config: &PluginConfig) -> Result<Self> {
        #[cfg(feature = "scripting")]
        let lua = Lua::new();

        let mut manager = Self {
            config: config.clone(),
            native_plugins: HashMap::new(),
            native_libraries: HashMap::new(),

            #[cfg(feature = "scripting")]
            lua_plugins: HashMap::new(),

            #[cfg(feature = "scripting")]
            lua,
        };

        manager.load_plugins()?;
        Ok(manager)
    }

    fn load_plugins(&mut self) -> Result<()> {
        let plugin_dir_path = self.config.plugin_dir.clone();
        let plugin_dir = Path::new(&plugin_dir_path);

        if !plugin_dir.exists() {
            warn!("Plugin directory does not exist: {:?}", plugin_dir);
            return Ok(());
        }

        let enabled_plugins = self.config.enabled.clone();
        for plugin_name in &enabled_plugins {
            info!("Loading plugin: {}", plugin_name);

            if let Err(e) = self.load_plugin(plugin_name, plugin_dir) {
                error!("Failed to load plugin {}: {}", plugin_name, e);
            }
        }

        Ok(())
    }

    fn load_plugin(&mut self, name: &str, plugin_dir: &Path) -> Result<()> {
        let native_path = plugin_dir.join(format!("lib{}.dylib", name));
        let lua_path = plugin_dir.join(format!("{}.lua", name));
        let js_path = plugin_dir.join(format!("{}.js", name));

        if native_path.exists() {
            self.load_native_plugin(name, &native_path)?;
        } else if lua_path.exists() {
            #[cfg(feature = "scripting")]
            self.load_lua_plugin(name, &lua_path)?;
            #[cfg(not(feature = "scripting"))]
            warn!(
                "Lua plugin found but scripting feature is disabled: {}",
                name
            );
        } else if js_path.exists() {
            warn!("JavaScript plugins not yet implemented: {}", name);
        } else {
            warn!("Plugin file not found for: {}", name);
        }

        Ok(())
    }

    fn load_native_plugin(&mut self, name: &str, path: &Path) -> Result<()> {
        debug!("Loading native plugin: {} from {:?}", name, path);

        unsafe {
            let lib = Library::new(path)?;

            type CreatePluginFn = unsafe fn() -> *mut dyn Plugin;
            let create_plugin: Symbol<CreatePluginFn> = lib.get(b"create_plugin")?;

            let plugin_ptr = create_plugin();
            if plugin_ptr.is_null() {
                return Err(anyhow::anyhow!("Plugin creation failed"));
            }

            let mut plugin = Box::from_raw(plugin_ptr);
            plugin.init()?;

            self.native_plugins.insert(name.to_string(), plugin);
            self.native_libraries.insert(name.to_string(), lib);
        }

        info!("Successfully loaded native plugin: {}", name);
        Ok(())
    }

    #[cfg(feature = "scripting")]
    fn load_lua_plugin(&mut self, name: &str, path: &Path) -> Result<()> {
        debug!("Loading Lua plugin: {} from {:?}", name, path);

        let script_content = std::fs::read_to_string(path)?;

        self.lua.load(&script_content).exec()?;

        if let Ok(init_fn) = self.lua.globals().get::<_, mlua::Function>("init") {
            init_fn.call::<_, ()>(())?;
        }

        let lua_plugin = LuaPlugin {
            name: name.to_string(),
            script_path: path.to_path_buf(),
        };

        self.lua_plugins.insert(name.to_string(), lua_plugin);

        info!("Successfully loaded Lua plugin: {}", name);
        Ok(())
    }

    pub fn on_window_created(&mut self, window: &Window) -> Result<()> {
        debug!("Notifying plugins of window creation: {}", window.title);

        for plugin in self.native_plugins.values_mut() {
            if let Err(e) = plugin.on_window_created(window) {
                error!("Plugin {} error on window created: {}", plugin.name(), e);
            }
        }

        #[cfg(feature = "scripting")]
        self.notify_lua_plugins("on_window_created", window)?;

        Ok(())
    }

    pub fn on_window_destroyed(&mut self, window: &Window) -> Result<()> {
        debug!("Notifying plugins of window destruction: {}", window.title);

        for plugin in self.native_plugins.values_mut() {
            if let Err(e) = plugin.on_window_destroyed(window) {
                error!("Plugin {} error on window destroyed: {}", plugin.name(), e);
            }
        }

        #[cfg(feature = "scripting")]
        self.notify_lua_plugins("on_window_destroyed", window)?;

        Ok(())
    }

    pub fn on_window_focused(&mut self, window_id: WindowId) -> Result<()> {
        debug!("Notifying plugins of window focus: {:?}", window_id);

        for plugin in self.native_plugins.values_mut() {
            if let Err(e) = plugin.on_window_focused(window_id) {
                error!("Plugin {} error on window focused: {}", plugin.name(), e);
            }
        }

        #[cfg(feature = "scripting")]
        self.notify_lua_plugins_window_focused(window_id)?;

        Ok(())
    }

    #[cfg(feature = "scripting")]
    fn notify_lua_plugins(&mut self, event: &str, _window: &Window) -> Result<()> {
        if let Ok(function) = self.lua.globals().get::<_, mlua::Function>(event) {
            if let Err(e) = function.call::<_, ()>(()) {
                error!("Lua plugin error on {}: {}", event, e);
            }
        }
        Ok(())
    }

    #[cfg(feature = "scripting")]
    fn notify_lua_plugins_window_focused(&mut self, window_id: WindowId) -> Result<()> {
        if let Ok(function) = self
            .lua
            .globals()
            .get::<_, mlua::Function>("on_window_focused")
        {
            if let Err(e) = function.call::<_, ()>(window_id.0) {
                error!("Lua plugin error on window focused: {}", e);
            }
        }
        Ok(())
    }

    pub fn reload_plugin(&mut self, name: &str) -> Result<()> {
        info!("Reloading plugin: {}", name);

        if let Some(mut plugin) = self.native_plugins.remove(name) {
            let _ = plugin.shutdown();
        }

        self.native_libraries.remove(name);

        #[cfg(feature = "scripting")]
        self.lua_plugins.remove(name);

        let plugin_dir_path = self.config.plugin_dir.clone();
        let plugin_dir = Path::new(&plugin_dir_path);
        self.load_plugin(name, plugin_dir)?;

        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<()> {
        info!("Shutting down plugin manager");

        for plugin in self.native_plugins.values_mut() {
            if let Err(e) = plugin.shutdown() {
                error!("Error shutting down plugin {}: {}", plugin.name(), e);
            }
        }

        self.native_plugins.clear();
        self.native_libraries.clear();

        #[cfg(feature = "scripting")]
        self.lua_plugins.clear();

        Ok(())
    }
}
