# Localhost Dashboard
![](https://camo.githubusercontent.com/0b67f2eb691b83144519058d27f3ae6104f24a760db25d4a0566c7c40f53731f/68747470733a2f2f696d672e736869656c64732e696f2f62616467652f727573742d2532333030303030302e7376673f7374796c653d666f722d7468652d6261646765266c6f676f3d72757374266c6f676f436f6c6f723d7768697465)

A simple dashboard showing which processes are running on which ports.

## Tech

- Rust
- [Rocket.rs](https://rocket.rs)
- Handlebars

## Features

- Lists all processes listening on ports
- Filters to dev ports (3000–9999 by default)
- Ignores noisy system processes (configurable)
- Attempts to resolve page titles from localhost
- Kill button per process
- `GET /json` for scripting
- Auto-refreshes every 10s, dark mode via `prefers-color-scheme`

## Requirements

- Rust (stable)
- macOS (uses native `listeners` and `sysinfo` libraries)

## Usage

```bash
cargo run --release
```

Runs on `http://localhost` (port 80 by default — may require `sudo`).

## Configuration

Edit `rocket.toml`:

```toml
[default]
address = "127.0.0.1"
port = 80

[default.dashboard]
ignored_processes = ["ControlCenter", "Raycast", ...]
dev_port_min = 3000
dev_port_max = 9999
```

## Run as a background service (macOS)

Build the release binary first:

```bash
cargo build --release
```

Install the included launchd plist:

```bash
cp com.petterkarlsrud.localhost-dashboard.plist ~/Library/LaunchAgents/
launchctl load ~/Library/LaunchAgents/com.petterkarlsrud.localhost-dashboard.plist
```

Useful commands:

```bash
launchctl stop com.petterkarlsrud.localhost-dashboard    # stop
launchctl start com.petterkarlsrud.localhost-dashboard   # start
launchctl unload ~/Library/LaunchAgents/com.petterkarlsrud.localhost-dashboard.plist  # disable
tail -f /tmp/localhost-dashboard.log                     # view logs
```
