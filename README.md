# Skew

A tiling window manager for macOS written in Rust, inspired by [sway](https://swaywm.org/) and [yabai](https://github.com/koekeishiya/yabai).

## Features

- **Advanced Tiling Layouts**: 7 layout algorithms (BSP, Stack, Grid, Spiral, Column, Monocle, Float)
- **Smart Focus Management**: Directional navigation, focus-follows-mouse, intelligent window filtering
- **Real macOS Integration**: Native Accessibility API bindings for window control and monitoring
- **Global Hotkeys**: System-wide keyboard shortcuts with customizable key bindings
- **Multi-Display Support**: Automatic display detection and per-monitor layout management
- **IPC Communication**: Full CLI-daemon communication for seamless control
- **Robust Configuration**: TOML, JSON, and YAML support with comprehensive validation
- **Plugin System**: Extensible with Lua scripting support
- **CLI Interface**: Complete command-line tools for control and automation
- **Production Ready**: Comprehensive error handling, logging, and clean architecture

## Project Structure

```
skew/
├── Cargo.toml              # Project configuration and dependencies
├── src/
│   ├── main.rs             # Main CLI entry point
│   ├── lib.rs              # Library exports and common types
│   ├── daemon.rs           # Background daemon service
│   ├── config.rs           # Configuration loading and management
│   ├── window_manager.rs   # Core window management logic
│   ├── layout.rs           # Window layout algorithms
│   ├── focus.rs            # Window focus management
│   ├── hotkeys.rs          # Keyboard shortcut handling
│   ├── ipc.rs              # Inter-process communication
│   ├── plugins.rs          # Plugin system and Lua integration
│   ├── bin/                # Additional binary targets
│   └── macos/              # macOS-specific implementations
│       ├── mod.rs
│       ├── accessibility.rs # Accessibility API bindings
│       ├── cgwindow.rs     # Core Graphics window operations
│       └── window_system.rs # Window system abstractions
└── target/                 # Build artifacts
```

## Installation

### Prerequisites

- **Rust**: Install from [rustup.rs](https://rustup.rs/)
- **macOS**: Requires macOS 10.12+ with Accessibility API support
- **Permissions**: **Critical** - Accessibility permissions must be granted for window management

### Granting Accessibility Permissions

1. Open **System Preferences** → **Security & Privacy** → **Privacy** → **Accessibility**
2. Click the lock icon and enter your password
3. Add the Skew binary to the list and enable it
4. Restart Skew after granting permissions

Without accessibility permissions, Skew will run in limited mode with reduced functionality.

### Build from Source

```bash
# Clone the repository
git clone https://github.com/plyght/skew.git
cd skew

# Build the project
cargo build --release

# Install binaries
cargo install --path .
```

## Usage

### Starting Skew

```bash
# Start with default configuration
skew start

# Start with custom config file
skew --config ~/.config/skew/custom.toml start
```

### Managing the Service

```bash
# Check status
skew status

# Reload configuration
skew reload

# Stop the service
skew stop
```

### Configuration

Default configuration location: `~/.config/skew/config.toml`

Example configuration:

```toml
[general]
gap = 10.0
border_width = 2.0
border_color = "#cccccc"
active_border_color = "#0080ff"

[layout]
default_layout = "bsp"  # bsp, stack, grid, spiral, column, monocle, float
split_ratio = 0.6

[focus]
follows_mouse = true
mouse_delay_ms = 100

[hotkeys]
mod_key = "alt"

[hotkeys.bindings]
"alt+h" = "focus_left"
"alt+j" = "focus_down"
"alt+k" = "focus_up"
"alt+l" = "focus_right"
"alt+shift+h" = "move_left"
"alt+shift+j" = "move_down"
"alt+shift+k" = "move_up"
"alt+shift+l" = "move_right"
"ctrl+alt+space" = "toggle_layout"
"alt+return" = "exec:terminal"
"alt+w" = "close_window"

[ipc]
socket_path = "/tmp/skew.sock"

[plugins]
enabled = []
plugin_dir = "~/.config/skew/plugins"
```

## Layout Algorithms

Skew supports 7 different tiling algorithms:

- **BSP (Binary Space Partitioning)**: Recursively splits screen space in half
- **Stack**: Master window on left, others stacked vertically on right
- **Grid**: Arranges windows in a grid pattern
- **Spiral**: First window takes main area, others spiral around it
- **Column**: All windows arranged in equal-width columns
- **Monocle**: Full-screen mode for focused window
- **Float**: Traditional floating window mode

Switch between layouts with `Ctrl+Alt+Space` or via IPC commands.

## Default Hotkeys

| Hotkey | Action |
|--------|--------|
| `Alt+H/J/K/L` | Focus window left/down/up/right |
| `Alt+Shift+H/J/K/L` | Move window left/down/up/right |
| `Ctrl+Alt+Space` | Toggle between layouts |
| `Alt+Enter` | Launch terminal |
| `Alt+W` | Close focused window |
| `Alt+Shift+Space` | Swap with main window |
| `Alt+Shift+R` | Restart/reload configuration |

All hotkeys are fully customizable in the configuration file.

## Development

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Check code formatting
cargo fmt --check

# Run clippy lints
cargo clippy
```

### Features

- `default`: Includes scripting support
- `scripting`: Lua plugin system (requires `mlua`)

```bash
# Build without scripting support
cargo build --no-default-features
```

## Architecture

### Components

- **Main CLI** (`main.rs`): Command-line interface and entry point
- **Daemon** (`daemon.rs`): Background service for window management
- **Window Manager** (`window_manager.rs`): Core logic for window operations
- **Layout Engine** (`layout.rs`): Algorithms for window arrangement
- **Plugin System** (`plugins.rs`): Lua scripting integration
- **IPC** (`ipc.rs`): Communication between CLI and daemon
- **macOS Integration** (`macos/`): Platform-specific window system bindings

### Binaries

- `skew`: Main CLI interface
- `skewd`: Background daemon service
- `skew-cli`: Additional command-line utilities

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Run `cargo test` and `cargo clippy`
6. Submit a pull request

## Acknowledgments

Skew is inspired by and builds upon ideas from:

- **[sway](https://swaywm.org/)**: A Wayland compositor and i3 compatible window manager for Linux
- **[yabai](https://github.com/koekeishiya/yabai)**: A tiling window manager for macOS based on binary space partitioning
- **[i3wm](https://i3wm.org/)**: The foundational tiling window manager that influenced many others

Special thanks to the maintainers and contributors of these projects for pioneering tiling window management.

## License

MIT License - see LICENSE file for details.

## Roadmap

### ✅ **Completed Features**
- [x] Complete Accessibility API integration
- [x] Implement hot-key handling with rdev
- [x] Add IPC communication between CLI and daemon
- [x] Develop comprehensive layout algorithms (7 layouts)
- [x] Add configuration validation
- [x] Implement window focus management
- [x] Add support for multiple displays
- [x] Implement proper directional focus with current window
- [x] Implement proper window movement
- [x] Get current focused window functionality
- [x] Implement application launching
- [x] Send stop/reload/status commands via IPC

### 🚧 **In Progress / Future Features**
- [ ] Enhanced global hotkey system (currently in simulation mode)
- [ ] Create plugin API documentation
- [ ] Add comprehensive test coverage
- [ ] Create installation scripts and homebrew formula
- [ ] Add window animations and smooth transitions
- [ ] Implement workspace/virtual desktop support
- [ ] Add window rules and application-specific configurations
- [ ] Create GUI configuration tool
- [ ] Add integration with popular macOS apps (Finder, Dock)
- [ ] Implement custom layout scripting
- [ ] Add performance monitoring and metrics
- [ ] Create comprehensive user documentation
- [ ] Add support for window shadows and visual effects
- [ ] Implement advanced gesture support
- [ ] Add backup and restore for configurations