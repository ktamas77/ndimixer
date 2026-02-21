use anyhow::Result;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, EventScreencastFrame, ScreencastFrameAckParams,
    StartScreencastFormat, StartScreencastParams, StopScreencastParams,
};
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
            .disable_default_args()
            .new_headless_mode()
            // Core args (from chromiumoxide defaults, minus --enable-automation which blocks autoplay)
            .arg("--disable-background-networking")
            .arg("--enable-features=NetworkService,NetworkServiceInProcess")
            .arg("--disable-background-timer-throttling")
            .arg("--disable-backgrounding-occluded-windows")
            .arg("--disable-breakpad")
            .arg("--disable-client-side-phishing-detection")
            .arg("--disable-component-extensions-with-background-pages")
            .arg("--disable-default-apps")
            .arg("--disable-dev-shm-usage")
            .arg("--disable-features=TranslateUI")
            .arg("--disable-hang-monitor")
            .arg("--disable-ipc-flooding-protection")
            .arg("--disable-popup-blocking")
            .arg("--disable-prompt-on-repost")
            .arg("--disable-renderer-backgrounding")
            .arg("--disable-sync")
            .arg("--force-color-profile=srgb")
            .arg("--metrics-recording-only")
            .arg("--no-first-run")
            .arg("--password-store=basic")
            .arg("--use-mock-keychain")
            .arg("--enable-blink-features=IdleDetection")
            .arg("--lang=en_US")
            // Our additions
            .arg("--no-sandbox")
            .arg("--autoplay-policy=no-user-gesture-required")
            .arg("--disable-blink-features=AutomationControlled")
            // Disable site isolation so evaluate_on_new_document runs in cross-origin iframes
            .arg("--disable-features=IsolateOrigins,site-per-process")
            .arg("--disable-site-isolation-trials")
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

        // Create blank page first, set up autoplay and viewport, then navigate
        let page = browser.new_page("about:blank").await?;

        // Set viewport size via CDP
        let metrics = SetDeviceMetricsOverrideParams::new(width as i64, height as i64, 1.0, false);
        page.execute(metrics).await?;

        // Register autoplay fix to run before any page JS on navigation
        let _ = page
            .evaluate_on_new_document(r#"
                // Force all media to autoplay by intercepting play() rejections
                const origPlay = HTMLMediaElement.prototype.play;
                HTMLMediaElement.prototype.play = function() {
                    this.muted = true;
                    return origPlay.call(this).catch(() => {
                        this.muted = true;
                        return origPlay.call(this);
                    });
                };
                // Auto-play any video/audio added to the DOM
                new MutationObserver((mutations) => {
                    for (const m of mutations) {
                        for (const node of m.addedNodes) {
                            if (node.nodeName === 'VIDEO' || node.nodeName === 'AUDIO') {
                                node.muted = true;
                                node.play().catch(() => {});
                            }
                            if (node.querySelectorAll) {
                                node.querySelectorAll('video, audio').forEach(el => {
                                    el.muted = true;
                                    el.play().catch(() => {});
                                });
                            }
                        }
                    }
                }).observe(document.documentElement, { childList: true, subtree: true });
                // Grant autoplay permission to all iframes (current and future)
                const grantAutoplay = (el) => {
                    if (el.tagName === 'IFRAME' && !el.allow.includes('autoplay')) {
                        el.allow = el.allow ? el.allow + '; autoplay' : 'autoplay; encrypted-media';
                    }
                };
                new MutationObserver((mutations) => {
                    for (const m of mutations) {
                        for (const node of m.addedNodes) {
                            if (node.nodeType === 1) {
                                grantAutoplay(node);
                                if (node.querySelectorAll) {
                                    node.querySelectorAll('iframe').forEach(grantAutoplay);
                                }
                            }
                        }
                        if (m.type === 'attributes' && m.attributeName === 'src' && m.target.tagName === 'IFRAME') {
                            grantAutoplay(m.target);
                        }
                    }
                }).observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ['src'] });
                // Also patch existing iframes at DOMContentLoaded
                document.addEventListener('DOMContentLoaded', () => {
                    document.body.style.background = 'transparent';
                    document.querySelectorAll('iframe').forEach(grantAutoplay);
                });
            "#)
            .await;

        // Now navigate to the actual URL
        page.goto(url).await?;

        // Simulate user clicks to establish "user activation" and hit any play buttons
        let center_x = width as f64 / 2.0;
        let center_y = height as f64 / 2.0;
        let _ = page
            .click(chromiumoxide::layout::Point {
                x: center_x,
                y: center_y,
            })
            .await;

        // Delayed click — Twitch embeds may take a moment to render their play button
        let page_ref = page.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let _ = page_ref
                .click(chromiumoxide::layout::Point {
                    x: center_x,
                    y: center_y,
                })
                .await;
            tokio::time::sleep(Duration::from_secs(3)).await;
            let _ = page_ref
                .click(chromiumoxide::layout::Point {
                    x: center_x,
                    y: center_y,
                })
                .await;
        });

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

        *loaded_ref.lock().unwrap() = true;
        tracing::info!("Browser overlay loaded: {}", url);

        let url_owned = url.to_string();

        let task = tokio::spawn(async move {
            if let Err(e) =
                capture_loop(page, &url_owned, width, height, reload_interval, frame_ref, cancel)
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

/// Take a screenshot with transparent background and store it.
async fn take_transparent_screenshot(
    page: &chromiumoxide::Page,
    latest_frame: &Arc<Mutex<Option<RgbaImage>>>,
) {
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
        Err(e) => tracing::warn!("Screenshot failed: {}", e),
    }
}

async fn capture_loop(
    page: chromiumoxide::Page,
    _url: &str,
    width: u32,
    height: u32,
    reload_interval: u64,
    latest_frame: Arc<Mutex<Option<RgbaImage>>>,
    cancel: CancellationToken,
) -> Result<()> {
    // Initial screenshot to prime the frame
    take_transparent_screenshot(&page, &latest_frame).await;

    // Start screencast as a change-detection mechanism. We don't use the screencast
    // frame data directly (it has broken alpha with transparent backgrounds). Instead,
    // when Chrome tells us content changed, we take a screenshot with omit_background.
    let mut stream = page.event_listener::<EventScreencastFrame>().await?;

    page.execute(
        StartScreencastParams::builder()
            .format(StartScreencastFormat::Png)
            .max_width(width as i64)
            .max_height(height as i64)
            .every_nth_frame(1)
            .build(),
    )
    .await?;

    tracing::info!("Screencast started ({}x{})", width, height);

    // Rate-limit screenshots to match output frame rate
    let min_interval = Duration::from_millis(33);
    let mut last_screenshot = tokio::time::Instant::now();

    // Periodic screenshot for content that loads after initial capture (React apps, etc.)
    let mut refresh_timer = tokio::time::interval(Duration::from_secs(2));
    refresh_timer.tick().await; // consume first tick

    let mut reload_timer = if reload_interval > 0 {
        Some(tokio::time::interval(Duration::from_secs(reload_interval)))
    } else {
        None
    };
    if let Some(ref mut timer) = reload_timer {
        timer.tick().await;
    }

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = page.execute(StopScreencastParams {}).await;
                tracing::info!("Screencast stopped (cancelled)");
                break;
            }

            // Reload interval
            _ = async {
                if let Some(ref mut timer) = reload_timer {
                    timer.tick().await
                } else {
                    std::future::pending::<tokio::time::Instant>().await
                }
            } => {
                tracing::debug!("Browser overlay reloading");
                let _ = page.execute(StopScreencastParams {}).await;
                let _ = page.reload().await;
                tokio::time::sleep(Duration::from_millis(500)).await;

                stream = page.event_listener::<EventScreencastFrame>().await?;
                page.execute(
                    StartScreencastParams::builder()
                        .format(StartScreencastFormat::Png)
                        .max_width(width as i64)
                        .max_height(height as i64)
                        .every_nth_frame(1)
                        .build(),
                ).await?;

                take_transparent_screenshot(&page, &latest_frame).await;
                last_screenshot = tokio::time::Instant::now();
                tracing::debug!("Screencast restarted after reload");
            }

            // Periodic refresh screenshot (catches late-loading content)
            _ = refresh_timer.tick() => {
                take_transparent_screenshot(&page, &latest_frame).await;
                last_screenshot = tokio::time::Instant::now();
            }

            // Screencast frame = content changed → take a screenshot
            frame_event = stream.next() => {
                match frame_event {
                    Some(event) => {
                        // Ack immediately so Chrome keeps sending change notifications
                        let _ = page.execute(ScreencastFrameAckParams::new(event.session_id)).await;

                        // Rate-limited screenshot for the actual frame data
                        if last_screenshot.elapsed() >= min_interval {
                            take_transparent_screenshot(&page, &latest_frame).await;
                            last_screenshot = tokio::time::Instant::now();
                        }
                    }
                    None => {
                        tracing::warn!("Screencast event stream ended");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}
