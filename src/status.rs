use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use std::sync::Arc;
use std::time::Instant;

use crate::channel::ChannelState;

#[derive(Serialize)]
struct StatusResponse {
    version: String,
    compositor: String,
    uptime_seconds: u64,
    channels: Vec<ChannelStatusJson>,
}

#[derive(Serialize)]
struct ChannelStatusJson {
    name: String,
    output_name: String,
    resolution: String,
    frame_rate: u32,
    ndi_input: Option<NdiInputStatus>,
    browser_overlays: Vec<BrowserOverlayStatus>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    filters: Vec<String>,
    frames_output: u64,
}

#[derive(Serialize)]
struct NdiInputStatus {
    source: String,
    connected: bool,
    frames_received: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    filters: Vec<String>,
}

#[derive(Serialize)]
struct BrowserOverlayStatus {
    url: String,
    loaded: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    filters: Vec<String>,
}

struct AppState {
    channels: Vec<Arc<ChannelState>>,
    compositor: String,
    start_time: Instant,
}

/// Start the HTTP status endpoint on the given port.
/// `channel_states` must be Arc-wrapped so they can be shared with the HTTP handler.
pub async fn serve_http(channel_states: Vec<Arc<ChannelState>>, compositor: &str, port: u16) -> anyhow::Result<()> {
    let state = Arc::new(AppState {
        channels: channel_states,
        compositor: compositor.to_string(),
        start_time: Instant::now(),
    });

    let app = Router::new()
        .route("/status", get(status_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!("Status endpoint: http://localhost:{}/status", port);

    axum::serve(listener, app).await?;
    Ok(())
}

async fn status_handler(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    let channels: Vec<ChannelStatusJson> = state
        .channels
        .iter()
        .map(|ch| {
            let ndi_input = ch.ndi_source.as_ref().map(|src| NdiInputStatus {
                source: src.clone(),
                connected: *ch.ndi_connected.lock().unwrap(),
                frames_received: *ch.ndi_frames_received.lock().unwrap(),
                filters: ch.ndi_filters.clone(),
            });

            let browser_overlays: Vec<BrowserOverlayStatus> = ch
                .browser_overlays
                .iter()
                .map(|b| BrowserOverlayStatus {
                    url: b.url.clone(),
                    loaded: *b.loaded.lock().unwrap(),
                    filters: b.filters.clone(),
                })
                .collect();

            ChannelStatusJson {
                name: ch.name.clone(),
                output_name: ch.output_name.clone(),
                resolution: format!("{}x{}", ch.width, ch.height),
                frame_rate: ch.frame_rate,
                ndi_input,
                browser_overlays,
                filters: ch.channel_filters.clone(),
                frames_output: *ch.frames_output.lock().unwrap(),
            }
        })
        .collect();

    Json(StatusResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        compositor: state.compositor.clone(),
        uptime_seconds: state.start_time.elapsed().as_secs(),
        channels,
    })
}
