use anyhow::Result;
use chromiumoxide::Browser;
use grafton_ndi::NDI;
use image::{ImageBuffer, Rgba, RgbaImage};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::browser::BrowserOverlay;
use crate::compositor::{self, Layer};
use crate::config::ChannelConfig;
use crate::ndi_input::NdiInput;
use crate::ndi_output::NdiOutput;

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
    _task: JoinHandle<()>,
}

impl Channel {
    pub async fn start(
        config: &ChannelConfig,
        ndi: &NDI,
        browser: Option<&Browser>,
        cancel: CancellationToken,
    ) -> Result<Self> {
        let width = config.width;
        let height = config.height;
        let frame_rate = config.frame_rate;
        let frame_interval = Duration::from_micros(1_000_000 / frame_rate as u64);

        // Start NDI input if configured
        let ndi_input = if let Some(ref ndi_cfg) = config.ndi_input {
            Some(NdiInput::start(ndi, &ndi_cfg.source, cancel.clone())?)
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
        let cancel_clone = cancel.clone();

        let task = tokio::spawn(async move {
            tracing::info!(
                "Channel '{}' started ({}x{}@{}fps)",
                channel_name,
                width,
                height,
                frame_rate
            );

            let mut interval = tokio::time::interval(frame_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            // Reusable canvas — allocated once, cleared each frame by compositor
            let mut canvas: RgbaImage =
                ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 255]));
            let num_browser = browser_layers.len();
            let mut layers = Vec::with_capacity(1 + num_browser);
            let mut ndi_output = ndi_output;

            // Keep last frames so we always have something to composite
            let mut last_ndi_frame: Option<RgbaImage> = None;
            let mut last_browser_frames: Vec<Option<RgbaImage>> = vec![None; num_browser];

            loop {
                tokio::select! {
                    _ = cancel_clone.cancelled() => break,
                    _ = interval.tick() => {
                        layers.clear();

                        // Take new NDI frame if available, keep last if not
                        if let Some(ref frame_lock) = ndi_latest {
                            if let Some(img) = take_frame(frame_lock) {
                                last_ndi_frame = Some(img);
                            }
                        }
                        if let Some(ref img) = last_ndi_frame {
                            layers.push(Layer {
                                image: img.clone(),
                                opacity: ndi_opacity,
                                z_index: ndi_z,
                            });
                        }

                        // Take new browser frames if available, keep last if not
                        for (i, (ref frame_lock, opacity, z_index)) in browser_layers.iter().enumerate() {
                            if let Some(img) = take_frame(frame_lock) {
                                last_browser_frames[i] = Some(img);
                            }
                            if let Some(ref img) = last_browser_frames[i] {
                                layers.push(Layer {
                                    image: img.clone(),
                                    opacity: *opacity,
                                    z_index: *z_index,
                                });
                            }
                        }

                        if layers.is_empty() {
                            // Canvas is already black from last clear, just send it
                            let _ = ndi_output.send_frame(&canvas);
                        } else {
                            compositor::composite(&mut canvas, &mut layers);
                            let _ = ndi_output.send_frame(&canvas);
                        }

                        *frames_output.lock().unwrap() += 1;
                    }
                }
            }

            tracing::info!("Channel '{}' stopped", channel_name);
        });

        Ok(Self {
            state: Arc::new(state),
            _task: task,
        })
    }
}
