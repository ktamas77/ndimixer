# Changelog

## v0.5.0
- **Per-layer GPU shader filters** — apply WGSL compute shader effects to individual layers or the final composited output (OBS ShaderFilter-inspired)
- Standard shader interface with time, dimensions, and up to 16 user-configurable float parameters
- Filters configurable per NDI input, per browser overlay, or per channel (post-processing)
- Five built-in filters: color_adjust, scanlines, chromatic_aberration, vignette, drop_shadow
- Filter status exposed in `/status` endpoint
- Shader compilation at startup with graceful error handling (broken shaders are skipped)
- CPU-only mode logs a warning and skips filters (no crash)
- Chrome anti-throttling flags for better WebSocket/timer support in headless overlays

## v0.4.0
- **30fps output achieved** — resolved performance bottleneck (was 8-12fps)
- Moved NDI input to dedicated OS threads with pre-resize to output dimensions
- Fixed macOS `thread::sleep` timer coalescing causing 50+ms oversleep — replaced with tight sleep loop + spin finish
- Pipelined NDI output on dedicated send thread (non-blocking `try_send`)
- Dedicated render threads (moved off tokio async runtime)
- Zero-copy layer compositing with borrowed image references

## v0.3.0
- Live FPS display in menu bar monitor
- Per-overlay browser URL display in menu bar dropdown
- GPU/CPU compositor mode indicator in status endpoint and menu bar

## v0.2.0
- GPU-accelerated compositing via wgpu Metal compute shaders
- Compositor mode (`gpu`/`cpu`) in HTTP status endpoint
- Terminal status shows GPU/CPU mode

## v0.1.0
- Initial release
- Multi-channel NDI mixing with HTML browser overlays
- CPU compositing with integer alpha blending
- HTTP status endpoint
- macOS menu bar monitor (Swift)
- Multiple browser overlays per channel
- launchd daemon support
