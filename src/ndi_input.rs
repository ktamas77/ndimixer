use anyhow::Result;
use grafton_ndi::{Finder, FinderOptions, Receiver, ReceiverColorFormat, ReceiverOptions, Source, NDI};
use image::{ImageBuffer, RgbaImage};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub struct NdiInput {
    pub latest_frame: Arc<Mutex<Option<RgbaImage>>>,
    pub connected: Arc<Mutex<bool>>,
    pub frames_received: Arc<Mutex<u64>>,
    _task: JoinHandle<()>,
}

impl NdiInput {
    pub fn start(
        ndi: &NDI,
        source_name: &str,
        cancel: CancellationToken,
    ) -> Result<Self> {
        let latest_frame: Arc<Mutex<Option<RgbaImage>>> = Arc::new(Mutex::new(None));
        let connected: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let frames_received: Arc<Mutex<u64>> = Arc::new(Mutex::new(0));

        let frame_ref = latest_frame.clone();
        let connected_ref = connected.clone();
        let frames_ref = frames_received.clone();
        let source_name = source_name.to_string();
        let ndi = ndi.clone();

        let task = tokio::spawn(async move {
            if let Err(e) = receive_loop(&ndi, &source_name, frame_ref, connected_ref, frames_ref, cancel).await {
                tracing::error!("NDI input '{}' error: {}", source_name, e);
            }
        });

        Ok(Self {
            latest_frame,
            connected,
            frames_received,
            _task: task,
        })
    }
}

async fn receive_loop(
    ndi: &NDI,
    source_name: &str,
    latest_frame: Arc<Mutex<Option<RgbaImage>>>,
    connected: Arc<Mutex<bool>>,
    frames_received: Arc<Mutex<u64>>,
    cancel: CancellationToken,
) -> Result<()> {
    tracing::info!("NDI input: searching for source '{}'...", source_name);

    // Find the source
    let source = find_source(ndi, source_name, &cancel).await?;
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
                    *latest_frame.lock().unwrap() = Some(img);
                    *frames_received.lock().unwrap() += 1;
                }
            }
            Ok(None) => {
                // Timeout, no frame available
                tokio::task::yield_now().await;
            }
            Err(e) => {
                tracing::warn!("NDI receive error: {}", e);
                *connected.lock().unwrap() = false;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }

    Ok(())
}

async fn find_source(ndi: &NDI, source_name: &str, cancel: &CancellationToken) -> Result<Source> {
    let finder_opts = FinderOptions::builder()
        .show_local_sources(true)
        .build();
    let finder = Finder::new(ndi, &finder_opts)?;

    loop {
        if cancel.is_cancelled() {
            anyhow::bail!("Cancelled while searching for NDI source '{}'", source_name);
        }

        let sources = finder.find_sources(Duration::from_secs(2))?;
        for source in &sources {
            if source.name.contains(source_name) {
                tracing::info!("NDI input: '{}' matched source '{}'", source_name, source.name);
                return Ok(source.clone());
            }
        }

        tracing::debug!("NDI source '{}' not found, retrying...", source_name);
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// List all NDI sources visible on the network.
pub fn list_sources(ndi: &NDI) -> Result<Vec<String>> {
    let finder_opts = FinderOptions::builder()
        .show_local_sources(true)
        .build();
    let finder = Finder::new(ndi, &finder_opts)?;

    println!("Searching for NDI sources (5 seconds)...");
    let sources = finder.find_sources(Duration::from_secs(5))?;

    let names: Vec<String> = sources.iter().map(|s| s.name.clone()).collect();
    Ok(names)
}
