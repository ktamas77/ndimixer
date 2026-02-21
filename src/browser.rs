use anyhow::Result;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;
use futures::StreamExt;
use image::RgbaImage;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Shared browser instance for all channels.
pub struct SharedBrowser {
    browser: Browser,
    _handler: JoinHandle<()>,
}

impl SharedBrowser {
    pub async fn launch() -> Result<Self> {
        let config = BrowserConfig::builder()
            .arg("--disable-gpu")
            .arg("--no-sandbox")
            .arg("--disable-dev-shm-usage")
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build browser config: {}", e))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to launch browser: {}", e))?;

        let handle = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if event.is_err() {
                    break;
                }
            }
        });

        tracing::info!("Headless browser launched");

        Ok(Self {
            browser,
            _handler: handle,
        })
    }

    pub fn browser(&self) -> &Browser {
        &self.browser
    }
}

/// Per-channel browser overlay that captures transparent screenshots.
pub struct BrowserOverlay {
    pub latest_frame: Arc<Mutex<Option<RgbaImage>>>,
    pub loaded: Arc<Mutex<bool>>,
    _task: JoinHandle<()>,
}

impl BrowserOverlay {
    pub async fn start(
        browser: &Browser,
        url: &str,
        width: u32,
        height: u32,
        css: &str,
        reload_interval: u64,
        cancel: CancellationToken,
    ) -> Result<Self> {
        let latest_frame: Arc<Mutex<Option<RgbaImage>>> = Arc::new(Mutex::new(None));
        let loaded: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));

        let frame_ref = latest_frame.clone();
        let loaded_ref = loaded.clone();

        let page = browser.new_page(url).await?;

        // Set viewport size via CDP
        let metrics = SetDeviceMetricsOverrideParams::new(width as i64, height as i64, 1.0, false);
        page.execute(metrics).await?;

        // Inject CSS if provided
        if !css.is_empty() {
            let js = format!(
                r#"
                const style = document.createElement('style');
                style.textContent = `{}`;
                document.head.appendChild(style);
                "#,
                css.replace('`', "\\`")
            );
            let _ = page.evaluate(js).await;
        }

        // Make page background transparent
        let _ = page
            .evaluate("document.body.style.background = 'transparent'")
            .await;

        *loaded_ref.lock().unwrap() = true;
        tracing::info!("Browser overlay loaded: {}", url);

        let url_owned = url.to_string();

        let task = tokio::spawn(async move {
            if let Err(e) = capture_loop(
                page,
                &url_owned,
                reload_interval,
                frame_ref,
                cancel,
            )
            .await
            {
                tracing::error!("Browser overlay error: {}", e);
            }
        });

        Ok(Self {
            latest_frame,
            loaded,
            _task: task,
        })
    }
}

async fn capture_loop(
    page: chromiumoxide::Page,
    _url: &str,
    reload_interval: u64,
    latest_frame: Arc<Mutex<Option<RgbaImage>>>,
    cancel: CancellationToken,
) -> Result<()> {
    let mut last_reload = tokio::time::Instant::now();

    loop {
        if cancel.is_cancelled() {
            break;
        }

        // Handle reload interval
        if reload_interval > 0
            && last_reload.elapsed() >= Duration::from_secs(reload_interval)
        {
            let _ = page.reload().await;
            last_reload = tokio::time::Instant::now();
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Capture screenshot with transparent background
        let params = ScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Png)
            .omit_background(true)
            .full_page(false)
            .build();

        match page.screenshot(params).await {
            Ok(png_data) => {
                if let Ok(img) = image::load_from_memory(&png_data) {
                    *latest_frame.lock().unwrap() = Some(img.to_rgba8());
                }
            }
            Err(e) => {
                tracing::warn!("Screenshot capture failed: {}", e);
            }
        }

        // ~60fps capture rate, the channel's frame rate controls output timing
        tokio::time::sleep(Duration::from_millis(16)).await;
    }

    Ok(())
}
