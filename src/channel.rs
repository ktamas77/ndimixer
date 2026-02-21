use anyhow::Result;
use chromiumoxide::Browser;
use grafton_ndi::NDI;
use image::{ImageBuffer, Rgba, RgbaImage};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use crate::browser::BrowserOverlay;
use crate::compositor::{self, Layer};
use crate::config::ChannelConfig;
use crate::ndi_input::NdiInput;
use crate::ndi_output::NdiOutput;

#[cfg(feature = "gpu")]
pub type GpuCtxParam = Option<Arc<crate::gpu_context::GpuContext>>;
#[cfg(not(feature = "gpu"))]
pub type GpuCtxParam = Option<Arc<()>>;

/// Take the latest frame from a shared buffer (zero-copy swap instead of clone).
fn take_frame(lock: &Mutex<Option<RgbaImage>>) -> Option<RgbaImage> {
    lock.lock().unwrap().take()
}

/// Per-overlay status info for reporting.
pub struct BrowserOverlayState {
    pub url: String,
    pub loaded: Arc<Mutex<bool>>,
}

/// Runtime state for a single channel, used for status reporting.
pub struct ChannelState {
    pub name: String,
    pub output_name: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: u32,
    pub ndi_connected: Arc<Mutex<bool>>,
    pub ndi_frames_received: Arc<Mutex<u64>>,
    pub ndi_source: Option<String>,
    pub browser_overlays: Vec<BrowserOverlayState>,
    pub frames_output: Arc<Mutex<u64>>,
}

pub struct Channel {
    pub state: Arc<ChannelState>,
    _thread: std::thread::JoinHandle<()>,
}

impl Channel {
    pub async fn start(
        config: &ChannelConfig,
        ndi: &NDI,
        browser: Option<&Browser>,
        gpu_ctx: GpuCtxParam,
        cancel: CancellationToken,
    ) -> Result<Self> {
        let width = config.width;
        let height = config.height;
        let frame_rate = config.frame_rate;
        let frame_interval = Duration::from_micros(1_000_000 / frame_rate as u64);

        // Start NDI input if configured (pre-resizes to output dims on its own thread)
        let ndi_input = if let Some(ref ndi_cfg) = config.ndi_input {
            Some(NdiInput::start(ndi, &ndi_cfg.source, width, height, cancel.clone())?)
        } else {
            None
        };

        // Start browser overlays
        let overlay_configs = config.all_browser_overlays();
        let mut browser_overlays = Vec::with_capacity(overlay_configs.len());
        for browser_cfg in &overlay_configs {
            let b = browser.ok_or_else(|| anyhow::anyhow!("Browser not available for overlay"))?;
            browser_overlays.push(
                BrowserOverlay::start(
                    b,
                    &browser_cfg.url,
                    browser_cfg.width,
                    browser_cfg.height,
                    &browser_cfg.css,
                    browser_cfg.reload_interval,
                    cancel.clone(),
                )
                .await?,
            );
        }

        // Create NDI output
        let ndi_output = NdiOutput::new(ndi, &config.output_name, width, height, frame_rate)?;

        // Build state for status reporting
        let ndi_connected = ndi_input
            .as_ref()
            .map(|i| i.connected.clone())
            .unwrap_or_else(|| Arc::new(Mutex::new(false)));
        let ndi_frames_received = ndi_input
            .as_ref()
            .map(|i| i.frames_received.clone())
            .unwrap_or_else(|| Arc::new(Mutex::new(0)));
        let frames_output: Arc<Mutex<u64>> = Arc::new(Mutex::new(0));

        let browser_overlay_states: Vec<BrowserOverlayState> = overlay_configs
            .iter()
            .zip(browser_overlays.iter())
            .map(|(cfg, overlay)| BrowserOverlayState {
                url: cfg.url.clone(),
                loaded: overlay.loaded.clone(),
            })
            .collect();

        let state = ChannelState {
            name: config.name.clone(),
            output_name: config.output_name.clone(),
            width,
            height,
            frame_rate,
            ndi_connected: ndi_connected.clone(),
            ndi_frames_received: ndi_frames_received.clone(),
            ndi_source: config.ndi_input.as_ref().map(|c| c.source.clone()),
            browser_overlays: browser_overlay_states,
            frames_output: frames_output.clone(),
        };

        // Layer z-index and opacity config
        let ndi_z = config.ndi_input.as_ref().map(|c| c.z_index).unwrap_or(0);
        let ndi_opacity = config.ndi_input.as_ref().map(|c| c.opacity).unwrap_or(1.0);

        // Collect browser overlay render info: (latest_frame_ref, opacity, z_index)
        let browser_layers: Vec<(Arc<Mutex<Option<RgbaImage>>>, f32, i32)> = overlay_configs
            .iter()
            .zip(browser_overlays.iter())
            .map(|(cfg, overlay)| {
                (overlay.latest_frame.clone(), cfg.opacity, cfg.z_index)
            })
            .collect();

        let ndi_latest = ndi_input.as_ref().map(|i| i.latest_frame.clone());

        let channel_name = config.name.clone();

        // Create per-channel GPU compositor if available
        #[cfg(feature = "gpu")]
        let mut gpu_compositor = gpu_ctx.map(|ctx| {
            crate::gpu_compositor::GpuCompositor::new(ctx, width, height)
        });

        // Suppress unused variable warning when gpu feature is off
        #[cfg(not(feature = "gpu"))]
        let _ = gpu_ctx;

        // Dedicated render thread — no async overhead, precise frame timing
        let thread = std::thread::Builder::new()
            .name(format!("render-{}", config.name))
            .spawn(move || {
                tracing::info!(
                    "Channel '{}' started ({}x{}@{}fps)",
                    channel_name,
                    width,
                    height,
                    frame_rate
                );

                let mut canvas: RgbaImage =
                    ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 255]));
                let num_browser = browser_layers.len();
                let mut ndi_output = ndi_output;

                let mut last_ndi_frame: Option<RgbaImage> = None;
                let mut last_browser_frames: Vec<Option<RgbaImage>> = vec![None; num_browser];

                loop {
                    let frame_start = Instant::now();

                    if cancel.is_cancelled() {
                        break;
                    }

                    // Take new frames into buffers
                    if let Some(ref frame_lock) = ndi_latest {
                        if let Some(img) = take_frame(frame_lock) {
                            last_ndi_frame = Some(img);
                        }
                    }
                    for (i, (ref frame_lock, _, _)) in browser_layers.iter().enumerate() {
                        if let Some(img) = take_frame(frame_lock) {
                            last_browser_frames[i] = Some(img);
                        }
                    }

                    // Build layer refs (no cloning)
                    let mut layers: Vec<Layer<'_>> = Vec::with_capacity(1 + num_browser);
                    if let Some(ref img) = last_ndi_frame {
                        layers.push(Layer {
                            image: img,
                            opacity: ndi_opacity,
                            z_index: ndi_z,
                        });
                    }
                    for (i, (_, opacity, z_index)) in browser_layers.iter().enumerate() {
                        if let Some(ref img) = last_browser_frames[i] {
                            layers.push(Layer {
                                image: img,
                                opacity: *opacity,
                                z_index: *z_index,
                            });
                        }
                    }

                    if layers.is_empty() {
                        let _ = ndi_output.send_frame(&canvas);
                    } else {
                        #[cfg(feature = "gpu")]
                        {
                            let used_gpu = if let Some(ref mut gpu) = gpu_compositor {
                                gpu.composite(&mut canvas, &mut layers)
                            } else {
                                false
                            };
                            if !used_gpu {
                                compositor::composite(&mut canvas, &mut layers);
                            }
                        }
                        #[cfg(not(feature = "gpu"))]
                        {
                            compositor::composite(&mut canvas, &mut layers);
                        }
                        let _ = ndi_output.send_frame(&canvas);
                    }

                    *frames_output.lock().unwrap() += 1;

                    // Precise frame timing: macOS timer coalescing causes thread::sleep
                    // to overshoot by 50+ms, so we use small sleep steps + spin finish.
                    if frame_start.elapsed() < frame_interval {
                        let target = frame_start + frame_interval;
                        loop {
                            let now = Instant::now();
                            if now >= target {
                                break;
                            }
                            let remaining = target - now;
                            if remaining > Duration::from_millis(3) {
                                std::thread::sleep(Duration::from_millis(1));
                            } else {
                                std::hint::spin_loop();
                            }
                        }
                    }
                }

                tracing::info!("Channel '{}' stopped", channel_name);
            })
            .expect("Failed to spawn render thread");

        Ok(Self {
            state: Arc::new(state),
            _thread: thread,
        })
    }
}
