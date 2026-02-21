use anyhow::Result;
use grafton_ndi::{
    Finder, FinderOptions, Receiver, ReceiverColorFormat, ReceiverOptions, Source, NDI,
};
use image::{ImageBuffer, RgbaImage};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub struct NdiInput {
    pub latest_frame: Arc<Mutex<Option<RgbaImage>>>,
    pub connected: Arc<Mutex<bool>>,
    pub frames_received: Arc<Mutex<u64>>,
    _thread: std::thread::JoinHandle<()>,
}

impl NdiInput {
    pub fn start(
        ndi: &NDI,
        source_name: &str,
        target_width: u32,
        target_height: u32,
        cancel: CancellationToken,
    ) -> Result<Self> {
        let latest_frame: Arc<Mutex<Option<RgbaImage>>> = Arc::new(Mutex::new(None));
        let connected: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let frames_received: Arc<Mutex<u64>> = Arc::new(Mutex::new(0));

        let frame_ref = latest_frame.clone();
        let connected_ref = connected.clone();
        let frames_ref = frames_received.clone();
        let name = source_name.to_string();
        let ndi = ndi.clone();

        let thread = std::thread::Builder::new()
            .name(format!("ndi-in-{}", source_name))
            .spawn(move || {
                if let Err(e) = receive_loop(
                    &ndi,
                    &name,
                    target_width,
                    target_height,
                    frame_ref,
                    connected_ref,
                    frames_ref,
                    cancel,
                ) {
                    tracing::error!("NDI input '{}' error: {}", name, e);
                }
            })
            .expect("Failed to spawn NDI input thread");

        Ok(Self {
            latest_frame,
            connected,
            frames_received,
            _thread: thread,
        })
    }
}

fn receive_loop(
    ndi: &NDI,
    source_name: &str,
    target_width: u32,
    target_height: u32,
    latest_frame: Arc<Mutex<Option<RgbaImage>>>,
    connected: Arc<Mutex<bool>>,
    frames_received: Arc<Mutex<u64>>,
    cancel: CancellationToken,
) -> Result<()> {
    tracing::info!("NDI input: searching for source '{}'...", source_name);

    // Find the source (blocking search on this dedicated thread)
    let source = find_source(ndi, source_name, &cancel)?;
    tracing::info!("NDI input: found source '{}'", source_name);

    // Create receiver with RGBA color format
    let recv_opts = ReceiverOptions::builder(source)
        .color(ReceiverColorFormat::RGBX_RGBA)
        .build();
    let receiver = Receiver::new(ndi, &recv_opts)?;

    *connected.lock().unwrap() = true;

    loop {
        if cancel.is_cancelled() {
            break;
        }

        // Poll for a video frame with short timeout
        match receiver.capture_video_timeout(Duration::from_millis(100)) {
            Ok(Some(frame)) => {
                let w = frame.width as u32;
                let h = frame.height as u32;

                if let Some(img) = ImageBuffer::from_raw(w, h, frame.data.clone()) {
                    // Resize to target dimensions once on this thread, not per-render-frame
                    let img = if w != target_width || h != target_height {
                        image::imageops::resize(
                            &img,
                            target_width,
                            target_height,
                            image::imageops::FilterType::Nearest,
                        )
                    } else {
                        img
                    };
                    *latest_frame.lock().unwrap() = Some(img);
                    *frames_received.lock().unwrap() += 1;
                }
            }
            Ok(None) => {
                // Timeout, no frame available — brief yield
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(e) => {
                tracing::warn!("NDI receive error: {}", e);
                *connected.lock().unwrap() = false;
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }

    Ok(())
}

fn find_source(ndi: &NDI, source_name: &str, cancel: &CancellationToken) -> Result<Source> {
    let finder_opts = FinderOptions::builder().show_local_sources(true).build();
    let finder = Finder::new(ndi, &finder_opts)?;

    loop {
        if cancel.is_cancelled() {
            anyhow::bail!("Cancelled while searching for NDI source '{}'", source_name);
        }

        let sources = finder.find_sources(Duration::from_secs(2))?;
        for source in &sources {
            if source.name.contains(source_name) {
                tracing::info!(
                    "NDI input: '{}' matched source '{}'",
                    source_name,
                    source.name
                );
                return Ok(source.clone());
            }
        }

        tracing::debug!("NDI source '{}' not found, retrying...", source_name);
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// List all NDI sources visible on the network.
pub fn list_sources(ndi: &NDI) -> Result<Vec<String>> {
    let finder_opts = FinderOptions::builder().show_local_sources(true).build();
    let finder = Finder::new(ndi, &finder_opts)?;

    println!("Searching for NDI sources (5 seconds)...");
    let sources = finder.find_sources(Duration::from_secs(5))?;

    let names: Vec<String> = sources.iter().map(|s| s.name.clone()).collect();
    Ok(names)
}
