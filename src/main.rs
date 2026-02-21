mod browser;
mod channel;
mod compositor;
mod config;
#[cfg(feature = "gpu")]
mod gpu_compositor;
#[cfg(feature = "gpu")]
mod gpu_context;
mod ndi_input;
mod ndi_output;
mod status;

use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use channel::ChannelState;

#[derive(Parser)]
#[command(
    name = "ndimixer",
    version,
    about = "Headless NDI mixer with HTML overlay support"
)]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,

    /// List available NDI sources and exit
    #[arg(long)]
    list_sources: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize NDI (needed for --list-sources before config is loaded)
    let ndi = grafton_ndi::NDI::new()?;

    // Handle --list-sources (no config needed)
    if cli.list_sources {
        let sources = ndi_input::list_sources(&ndi)?;
        if sources.is_empty() {
            println!("No NDI sources found.");
        } else {
            println!(
                "Found {} NDI source{}:",
                sources.len(),
                if sources.len() == 1 { "" } else { "s" }
            );
            for source in &sources {
                println!("  - {}", source);
            }
        }
        return Ok(());
    }

    // Load config
    let config = config::Config::load(&cli.config)?;

    // Initialize logging with level from config
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&config.settings.log_level)),
        )
        .init();

    tracing::info!(
        "NDI Mixer v{} starting with {} channel{}",
        env!("CARGO_PKG_VERSION"),
        config.channel.len(),
        if config.channel.len() == 1 { "" } else { "s" }
    );

    let cancel = CancellationToken::new();

    // Launch shared browser if any channel needs it
    let shared_browser = if config.has_browser_overlays() {
        tracing::info!("Launching headless browser for overlays...");
        Some(browser::SharedBrowser::launch().await?)
    } else {
        None
    };

    // Initialize GPU compositor if feature enabled
    #[cfg(feature = "gpu")]
    let gpu_ctx = gpu_context::GpuContext::try_new();
    #[cfg(not(feature = "gpu"))]
    let gpu_ctx: Option<std::sync::Arc<()>> = None;

    // Start channels
    let mut channels = Vec::new();
    for ch_config in &config.channel {
        let ch = channel::Channel::start(
            ch_config,
            &ndi,
            shared_browser.as_ref().map(|b| b.browser()),
            gpu_ctx.clone(),
            cancel.clone(),
        )
        .await?;
        channels.push(ch);
    }

    // Collect Arc<ChannelState> for shared access
    let channel_states: Vec<Arc<ChannelState>> =
        channels.iter().map(|ch| ch.state.clone()).collect();

    // Determine compositor mode label
    let compositor_mode: &str;
    #[cfg(feature = "gpu")]
    {
        compositor_mode = if gpu_ctx.is_some() { "gpu" } else { "cpu" };
    }
    #[cfg(not(feature = "gpu"))]
    {
        compositor_mode = "cpu";
    }

    // Start HTTP status endpoint if configured
    let status_port = config.settings.status_port;
    if status_port > 0 {
        let states_for_http = channel_states.clone();
        let compositor_str = compositor_mode.to_string();
        tokio::spawn(async move {
            if let Err(e) = status::serve_http(states_for_http, &compositor_str, status_port).await {
                tracing::error!("Status HTTP server error: {}", e);
            }
        });
        println!("Status: http://localhost:{}/status", status_port);
    }

    // Ctrl+C handler
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Shutting down...");
        cancel_clone.cancel();
    });

    // Print status periodically until cancelled
    loop {
        if cancel.is_cancelled() {
            break;
        }
        print_terminal_status(&channel_states, compositor_mode);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    println!("\nNDI Mixer stopped.");
    Ok(())
}

fn print_terminal_status(channels: &[Arc<ChannelState>], compositor: &str) {
    print!("\x1b[2J\x1b[H"); // Clear screen, cursor to top
    println!(
        "NDI Mixer v{} — {} channel{} active ({})\n",
        env!("CARGO_PKG_VERSION"),
        channels.len(),
        if channels.len() == 1 { "" } else { "s" },
        compositor.to_uppercase()
    );

    for ch in channels {
        let ndi_status = if let Some(ref src) = ch.ndi_source {
            let connected = *ch.ndi_connected.lock().unwrap();
            if connected {
                format!("NDI: \x1b[32m+\x1b[0m {}", src)
            } else {
                format!("NDI: \x1b[33m~\x1b[0m {}", src)
            }
        } else {
            "NDI: -".to_string()
        };

        let browser_status = if ch.browser_overlays.is_empty() {
            "Browser: -".to_string()
        } else {
            let loaded_count = ch
                .browser_overlays
                .iter()
                .filter(|b| *b.loaded.lock().unwrap())
                .count();
            let total = ch.browser_overlays.len();
            if loaded_count == total {
                if total == 1 {
                    "Browser: \x1b[32m+\x1b[0m loaded".to_string()
                } else {
                    format!("Browser: \x1b[32m+\x1b[0m {} loaded", total)
                }
            } else {
                format!(
                    "Browser: \x1b[33m~\x1b[0m {}/{} loaded",
                    loaded_count, total
                )
            }
        };

        let frames = *ch.frames_output.lock().unwrap();

        println!(
            "  {:<16} {}  |  {}  |  Out: {} ({}x{}@{}) [{}f]",
            ch.name,
            ndi_status,
            browser_status,
            ch.output_name,
            ch.width,
            ch.height,
            ch.frame_rate,
            frames
        );
    }
    println!();
}
