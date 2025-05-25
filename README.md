# Skew

A tiling window manager for macOS written in Rust.

## Features

- **Tiling Window Management**: Automatically arrange windows in efficient layouts
- **Plugin System**: Extensible with Lua scripting support
- **macOS Integration**: Native macOS window system support via Accessibility APIs
- **Configuration**: TOML, JSON, and YAML configuration file support
- **CLI Interface**: Command-line tools for control and automation
- **Daemon Mode**: Background service with IPC communication

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
- **macOS**: Requires macOS with Accessibility API support
- **Permissions**: Accessibility permissions required for window management

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
default_layout = "bsp"
gap_size = 10

[layouts]
bsp_ratio = 0.6

[hotkeys]
mod_key = "cmd"
```

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

## License

MIT License - see LICENSE file for details.

## TODO

- [ ] Complete Accessibility API integration
- [ ] Implement hot-key handling with rdev
- [ ] Add IPC communication between CLI and daemon
- [ ] Develop comprehensive layout algorithms
- [ ] Create plugin API documentation
- [ ] Add configuration validation
- [ ] Implement window focus management
- [ ] Add support for multiple displays
- [ ] Create installation scripts
- [ ] Add comprehensive test coverage