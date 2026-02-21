# NDI Mixer

A lightweight, headless NDI mixer that composites NDI video sources with HTML/browser overlays and outputs the result as NDI streams. Designed to run in the background as an alternative to OBS for multi-channel NDI mixing — without the throttling OBS applies to non-active scenes.

## Why?

OBS throttles rendering on non-active scenes/mixes. If you need multiple simultaneous NDI composites (e.g., for multi-camera streaming setups), OBS becomes a bottleneck. NDI Mixer runs each channel independently at full frame rate, compositing NDI inputs with HTML overlays and sending the result to any NDI receiver on your network.

## Features

- **Multi-channel mixing** — define as many channels as you need, each with independent settings
- **NDI input** — receive any NDI source as a video layer (zero or one per channel)
- **HTML/browser overlay** — render any URL as a transparent overlay layer (zero or one per channel)
- **Per-channel output resolution** — configure each channel's output size (e.g., 1920x1080, 1280x720)
- **Per-layer browser resolution** — the browser overlay can render at a different resolution than the output
- **Layer ordering** — control z-index of NDI input vs. browser overlay
- **Layer opacity** — per-layer opacity control (0.0–1.0)
- **Transparent HTML support** — HTML pages with transparent backgrounds composite correctly (like OBS browser sources)
- **NDI output per channel** — each channel outputs its own NDI stream
- **Config-file driven** — single TOML config file defines all channels and settings
- **Headless operation** — runs in the background with terminal status display
- **Status endpoint** — optional HTTP GET endpoint for monitoring (configurable port)

## Architecture

```
┌─────────────────────────────────────────────────┐
│                   NDI Mixer                      │
│                                                  │
│  ┌─────────── Channel "Main" ──────────────┐    │
│  │                                          │    │
│  │  ┌──────────┐    ┌──────────────────┐   │    │
│  │  │ NDI In   │    │ Browser Overlay  │   │    │
│  │  │ (camera) │    │ (HTML/CSS/JS)    │   │    │
│  │  │ z:0 α:1  │    │ z:1 α:0.8       │   │    │
│  │  └────┬─────┘    └───────┬──────────┘   │    │
│  │       │                  │              │    │
│  │       └──────┬───────────┘              │    │
│  │              ▼                          │    │
│  │        ┌───────────┐                    │    │
│  │        │ Composite │                    │    │
│  │        │ 1920x1080 │                    │    │
│  │        └─────┬─────┘                    │    │
│  │              ▼                          │    │
│  │        ┌───────────┐                    │    │
│  │        │ NDI Out   │                    │    │
│  │        │ "Mixer-   │                    │    │
│  │        │  Main"    │                    │    │
│  │        └───────────┘                    │    │
│  └──────────────────────────────────────────┘    │
│                                                  │
│  ┌─────────── Channel "PiP" ───────────────┐    │
│  │  ...                                     │    │
│  └──────────────────────────────────────────┘    │
│                                                  │
│  ┌─ Status Endpoint (optional) ─┐               │
│  │  GET http://localhost:9100    │               │
│  └──────────────────────────────┘               │
└─────────────────────────────────────────────────┘
```

## Configuration

Copy the example config and edit it for your setup:

```bash
cp config.example.toml config.toml
```

NDI Mixer is configured via a single `config.toml` file.

### Example

```toml
# Global settings
[settings]
status_port = 9100           # Optional HTTP status endpoint (0 = disabled)
log_level = "info"           # debug, info, warn, error

# Channel definitions
[[channel]]
name = "Main"
output_name = "Mixer-Main"   # NDI output name visible on the network
width = 1920                 # Output resolution
height = 1080
frame_rate = 30              # Output frame rate

  [channel.ndi_input]
  source = "MY-PC (Camera)"  # NDI source name to receive
  z_index = 0                # Layer order (lower = behind)
  opacity = 1.0              # Layer opacity (0.0 - 1.0)

  [channel.browser_overlay]
  url = "https://example.com/overlay.html"
  width = 1920               # Browser render resolution
  height = 1080
  z_index = 1                # Rendered on top of NDI input
  opacity = 0.8
  css = ""                   # Optional injected CSS
  reload_interval = 0        # Auto-reload in seconds (0 = disabled)

[[channel]]
name = "Clean Feed"
output_name = "Mixer-Clean"
width = 1920
height = 1080
frame_rate = 30

  [channel.ndi_input]
  source = "MY-PC (Camera)"
  z_index = 0
  opacity = 1.0

  # No browser overlay — clean feed, NDI passthrough
```

### Configuration Reference

#### `[settings]`

| Field         | Type   | Default | Description                                    |
|---------------|--------|---------|------------------------------------------------|
| `status_port` | int    | `0`     | HTTP status endpoint port. `0` to disable.     |
| `log_level`   | string | `info`  | Log level: `debug`, `info`, `warn`, `error`    |

#### `[[channel]]`

| Field         | Type   | Required | Description                                  |
|---------------|--------|----------|----------------------------------------------|
| `name`        | string | yes      | Human-readable channel name                  |
| `output_name` | string | yes      | NDI output name visible on the network       |
| `width`       | int    | yes      | Output width in pixels                       |
| `height`      | int    | yes      | Output height in pixels                      |
| `frame_rate`  | int    | `30`     | Output frame rate                            |

#### `[channel.ndi_input]` (optional)

| Field      | Type   | Required | Description                           |
|------------|--------|----------|---------------------------------------|
| `source`   | string | yes      | NDI source name (substring match — see below) |
| `z_index`  | int    | `0`      | Layer draw order (lower = behind)     |
| `opacity`  | float  | `1.0`    | Layer opacity (0.0–1.0)              |

**NDI source matching:** The `source` field uses substring matching — you don't need to specify the full NDI source name. For example, `"Synesthesia"` will match `"MY-PC (Synesthesia)"`. The full matched source name is logged at startup. Use `--list-sources` to see all available NDI names on your network.

#### `[channel.browser_overlay]` (optional)

| Field              | Type   | Required | Description                              |
|--------------------|--------|----------|------------------------------------------|
| `url`              | string | yes      | HTTP/HTTPS URL to render                 |
| `width`            | int    | yes      | Browser viewport width                   |
| `height`           | int    | yes      | Browser viewport height                  |
| `z_index`          | int    | `1`      | Layer draw order (lower = behind)        |
| `opacity`          | float  | `1.0`    | Layer opacity (0.0–1.0)                 |
| `css`              | string | `""`     | CSS to inject into the page              |
| `reload_interval`  | int    | `0`      | Auto-reload interval in seconds (0=off)  |

## Technology

| Component          | Technology                                                            |
|--------------------|-----------------------------------------------------------------------|
| Language           | Rust                                                                  |
| NDI send/receive   | [grafton-ndi](https://github.com/GrantSparks/grafton-ndi) (NDI 6 SDK)|
| HTML rendering     | Headless Chromium via [chromiumoxide](https://github.com/mattsse/chromiumoxide) |
| Compositing        | Custom RGBA alpha blending (SIMD-optimized)                           |
| Config             | TOML via [toml](https://crates.io/crates/toml) + [serde](https://serde.rs) |
| HTTP status        | [axum](https://github.com/tokio-rs/axum) (lightweight)               |
| Async runtime      | [tokio](https://tokio.rs)                                            |

### Why Rust?

- **Real-time performance** — compositing 1080p60 RGBA frames requires sub-millisecond blending; Rust delivers this with zero-copy NDI receive and SIMD-capable compositing
- **grafton-ndi** — the most actively maintained NDI binding across any language, supporting NDI 6 with async/await and zero-copy receive
- **Memory safety** — no GC pauses disrupting frame timing
- **Low resource usage** — ideal for a background service

## Prerequisites

- **NDI 6 SDK** — runtime library (`libndi`) required
- **Rust toolchain** — for building from source
- **Google Chrome or Chromium** — required for HTML overlay rendering

## Installation (macOS)

### 1. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 2. Install NDI Tools (includes NDI runtime)

Download and install from [ndi.video](https://ndi.video/for-developers/ndi-sdk/download/), or use the direct installer:

```bash
open https://downloads.ndi.tv/Tools/NDIToolsInstaller.pkg
```

If you get `libndi.dylib` errors after install, fix permissions and re-run the installer:

```bash
sudo mkdir -p /usr/local/lib
sudo chmod 755 /usr/local/lib
```

Verify the library is installed:

```bash
ls /usr/local/lib/libndi.dylib
```

### 3. Install Chrome

If not already installed, download from [google.com/chrome](https://www.google.com/chrome/) or:

```bash
brew install --cask google-chrome
```

### 4. Build

```bash
git clone https://github.com/ktamas77/ndimixer.git
cd ndimixer
cargo build --release
```

### 5. Configure

```bash
cp config.example.toml config.toml
# Edit config.toml with your NDI sources and overlay URLs
```

## Usage

```bash
# Run (macOS requires DYLD_LIBRARY_PATH for NDI)
DYLD_LIBRARY_PATH=/usr/local/lib ./target/release/ndimixer

# Run with a specific config file
DYLD_LIBRARY_PATH=/usr/local/lib ./target/release/ndimixer --config /path/to/config.toml

# List available NDI sources on the network
DYLD_LIBRARY_PATH=/usr/local/lib ./target/release/ndimixer --list-sources
```

**Tip:** Add this to your `~/.zshrc` to avoid typing it every time:

```bash
export DYLD_LIBRARY_PATH="/usr/local/lib:$DYLD_LIBRARY_PATH"
```

### Terminal Output

When running, NDI Mixer displays a live status in the terminal:

```
NDI Mixer v0.1.0 — 2 channels active

  Main         NDI: ✓ MY-PC (Camera)  |  Browser: ✓ loaded  |  Output: Mixer-Main (1920x1080@30)
  Clean Feed   NDI: ✓ MY-PC (Camera)  |  Browser: —         |  Output: Mixer-Clean (1920x1080@30)

Status: http://localhost:9100
```

### HTTP Status Endpoint

When `status_port` is configured, a JSON status endpoint is available:

```bash
curl http://localhost:9100/status
```

```json
{
  "version": "0.1.0",
  "uptime_seconds": 3421,
  "channels": [
    {
      "name": "Main",
      "output_name": "Mixer-Main",
      "resolution": "1920x1080",
      "frame_rate": 30,
      "ndi_input": {
        "source": "MY-PC (Camera)",
        "connected": true,
        "frames_received": 102630
      },
      "browser_overlay": {
        "url": "https://example.com/overlay.html",
        "loaded": true
      },
      "frames_output": 102628
    }
  ]
}
```

## Menu Bar Monitor (macOS)

A lightweight macOS menu bar app that shows NDI Mixer status at a glance. Runs independently — works whether ndimixer is running or not.

### Build

```bash
cd monitor
bash build.sh
```

### Run

```bash
# Default (connects to http://localhost:9100/status)
./monitor/NDIMixerMonitor &

# Custom endpoint
./monitor/NDIMixerMonitor --url http://192.168.1.50:9100/status &
```

A green **NDI** label appears in the menu bar when connected. Click it to see per-channel status (NDI input, browser overlay, output resolution, frame count). When ndimixer is not running, the label turns gray and the dropdown shows "NDI Mixer: not running".

## Roadmap

- [x] Core NDI receive and send pipeline
- [x] TOML config parsing
- [x] RGBA compositing with alpha blending
- [x] Headless Chromium HTML overlay rendering
- [x] Per-layer opacity and z-index
- [x] Terminal status display
- [x] HTTP status endpoint
- [x] Zero-copy NDI send (BorrowedVideoFrame)
- [x] Reusable frame buffers (no per-frame allocation)
- [x] Integer-based compositing (u16 fast path)
- [x] macOS menu bar monitor (Swift)
- [ ] Hot-reload config (SIGHUP or file watch)
- [ ] Audio passthrough from NDI input
- [ ] Multiple NDI inputs per channel
- [ ] Multiple browser overlays per channel
- [ ] GPU-accelerated compositing (wgpu)

## License

MIT License — see [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please open an issue to discuss significant changes before submitting a PR.
