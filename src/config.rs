use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub channel: Vec<ChannelConfig>,
}

#[derive(Debug, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub status_port: u16,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            status_port: 0,
            log_level: "info".to_string(),
        }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

#[derive(Debug, Deserialize)]
pub struct ChannelConfig {
    pub name: String,
    pub output_name: String,
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_frame_rate")]
    pub frame_rate: u32,
    pub ndi_input: Option<NdiInputConfig>,
    pub browser_overlay: Option<BrowserOverlayConfig>,
}

fn default_frame_rate() -> u32 {
    30
}

#[derive(Debug, Deserialize)]
pub struct NdiInputConfig {
    pub source: String,
    #[serde(default)]
    pub z_index: i32,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
}

#[derive(Debug, Deserialize)]
pub struct BrowserOverlayConfig {
    pub url: String,
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_z_index_overlay")]
    pub z_index: i32,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub css: String,
    #[serde(default)]
    pub reload_interval: u64,
}

fn default_opacity() -> f32 {
    1.0
}

fn default_z_index_overlay() -> i32 {
    1
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read config file {}: {}", path.display(), e))?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse config file: {}", e))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.channel.is_empty() {
            anyhow::bail!("At least one channel must be defined");
        }
        for ch in &self.channel {
            if ch.width == 0 || ch.height == 0 {
                anyhow::bail!("Channel '{}': width and height must be > 0", ch.name);
            }
            if ch.frame_rate == 0 {
                anyhow::bail!("Channel '{}': frame_rate must be > 0", ch.name);
            }
            if let Some(ref ndi) = ch.ndi_input {
                if !(0.0..=1.0).contains(&ndi.opacity) {
                    anyhow::bail!("Channel '{}': ndi_input opacity must be 0.0–1.0", ch.name);
                }
            }
            if let Some(ref browser) = ch.browser_overlay {
                if browser.width == 0 || browser.height == 0 {
                    anyhow::bail!(
                        "Channel '{}': browser width and height must be > 0",
                        ch.name
                    );
                }
                if !(0.0..=1.0).contains(&browser.opacity) {
                    anyhow::bail!(
                        "Channel '{}': browser_overlay opacity must be 0.0–1.0",
                        ch.name
                    );
                }
            }
        }
        Ok(())
    }

    pub fn has_browser_overlays(&self) -> bool {
        self.channel.iter().any(|ch| ch.browser_overlay.is_some())
    }
}
