use serde::Deserialize;
use std::collections::HashMap;
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

#[derive(Debug, Clone, Deserialize)]
pub struct FilterConfig {
    pub shader: String,
    #[serde(default)]
    pub params: HashMap<String, f32>,
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
    /// Legacy single overlay (backwards compat with `[channel.browser_overlay]`)
    #[serde(default)]
    browser_overlay: Option<BrowserOverlayConfig>,
    /// Multiple overlays (`[[channel.browser_overlays]]`)
    #[serde(default)]
    browser_overlays: Vec<BrowserOverlayConfig>,
    /// Channel-level post-processing filters (applied after all layers composited)
    #[serde(default)]
    pub filters: Vec<FilterConfig>,
}

impl ChannelConfig {
    /// Returns all browser overlays, merging legacy single `browser_overlay` with `browser_overlays`.
    pub fn all_browser_overlays(&self) -> Vec<&BrowserOverlayConfig> {
        let mut all: Vec<&BrowserOverlayConfig> = Vec::new();
        if let Some(ref single) = self.browser_overlay {
            all.push(single);
        }
        all.extend(self.browser_overlays.iter());
        all
    }
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
    #[serde(default)]
    pub filters: Vec<FilterConfig>,
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
    #[serde(default)]
    pub filters: Vec<FilterConfig>,
}

fn default_opacity() -> f32 {
    1.0
}

fn default_z_index_overlay() -> i32 {
    1
}

fn validate_filter(filter: &FilterConfig, channel: &str, layer: &str) -> anyhow::Result<()> {
    if !Path::new(&filter.shader).exists() {
        anyhow::bail!(
            "Channel '{}': {} filter shader not found: {}",
            channel,
            layer,
            filter.shader
        );
    }
    if filter.params.len() > 16 {
        anyhow::bail!(
            "Channel '{}': {} filter has {} params (max 16)",
            channel,
            layer,
            filter.params.len()
        );
    }
    Ok(())
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
                for filter in &ndi.filters {
                    validate_filter(filter, &ch.name, "ndi_input")?;
                }
            }
            for filter in &ch.filters {
                validate_filter(filter, &ch.name, "channel")?;
            }
            for browser in ch.all_browser_overlays() {
                if browser.width == 0 || browser.height == 0 {
                    anyhow::bail!(
                        "Channel '{}': browser overlay width and height must be > 0",
                        ch.name
                    );
                }
                if !(0.0..=1.0).contains(&browser.opacity) {
                    anyhow::bail!(
                        "Channel '{}': browser overlay opacity must be 0.0–1.0",
                        ch.name
                    );
                }
                for filter in &browser.filters {
                    validate_filter(filter, &ch.name, "browser_overlay")?;
                }
            }
        }
        Ok(())
    }

    pub fn has_browser_overlays(&self) -> bool {
        self.channel
            .iter()
            .any(|ch| !ch.all_browser_overlays().is_empty())
    }
}
