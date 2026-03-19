use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::Cli;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub device: String,
    pub output_size: Size,
    pub initial_size: Size,
    pub fps: u32,
    pub border_color: String,
    pub border_width: u32,
    pub portal_restore_token: Option<String>,
    pub presets: Vec<Size>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split('x').collect();
        if parts.len() != 2 {
            eyre::bail!("Invalid size format '{}', expected WxH (e.g., 1280x720)", s);
        }
        let width: u32 = parts[0].parse().context(format!("Invalid width in '{}'", s))?;
        let height: u32 = parts[1].parse().context(format!("Invalid height in '{}'", s))?;
        Ok(Self { width, height })
    }
}

impl std::fmt::Display for Size {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device: "/dev/video10".to_string(),
            output_size: Size::new(1280, 720),
            initial_size: Size::new(1280, 720),
            fps: 30,
            border_color: "#ff3333".to_string(),
            border_width: 4,
            portal_restore_token: None,
            presets: vec![Size::new(1280, 720), Size::new(1920, 1080), Size::new(960, 540)],
        }
    }
}

impl Config {
    pub fn load(cli: &Cli) -> Result<Self> {
        let mut config = if let Some(path) = &cli.config {
            Self::load_from_file(path).context(format!("Failed to load config from {}", path.display()))?
        } else {
            Self::load_from_default_locations()
        };

        // CLI overrides
        if let Some(device) = &cli.device {
            config.device = device.clone();
        }
        if let Some(size) = &cli.size {
            let parsed = Size::parse(size)?;
            config.initial_size = parsed;
        }
        if let Some(fps) = cli.fps {
            config.fps = fps;
        }
        if let Some(color) = &cli.color {
            config.border_color = color.clone();
        }
        if let Some(border_width) = cli.border_width {
            config.border_width = border_width;
        }

        Ok(config)
    }

    fn load_from_default_locations() -> Self {
        if let Some(config_dir) = dirs::config_dir() {
            let project_name = env!("CARGO_PKG_NAME");
            let primary_config = config_dir.join(project_name).join(format!("{}.yml", project_name));
            if primary_config.exists() {
                match Self::load_from_file(&primary_config) {
                    Ok(config) => return config,
                    Err(e) => {
                        log::warn!("Failed to load config from {}: {}", primary_config.display(), e);
                    }
                }
            }
        }

        let project_name = env!("CARGO_PKG_NAME");
        let fallback_config = PathBuf::from(format!("{}.yml", project_name));
        if fallback_config.exists() {
            match Self::load_from_file(&fallback_config) {
                Ok(config) => return config,
                Err(e) => {
                    log::warn!("Failed to load config from {}: {}", fallback_config.display(), e);
                }
            }
        }

        log::info!("No config file found, using defaults");
        Self::default()
    }

    fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path).context("Failed to read config file")?;
        let config: Self = serde_yaml::from_str(&content).context("Failed to parse config file")?;
        log::info!("Loaded config from: {}", path.as_ref().display());
        Ok(config)
    }

    #[allow(dead_code)]
    pub fn save(&self) -> Result<()> {
        if let Some(config_dir) = dirs::config_dir() {
            let project_name = env!("CARGO_PKG_NAME");
            let config_path = config_dir.join(project_name).join(format!("{}.yml", project_name));
            if let Some(parent) = config_path.parent() {
                fs::create_dir_all(parent).context("Failed to create config directory")?;
            }
            let content = serde_yaml::to_string(self).context("Failed to serialize config")?;
            fs::write(&config_path, content).context("Failed to write config file")?;
            log::info!("Saved config to: {}", config_path.display());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_parse_valid() {
        let size = Size::parse("1280x720").expect("should parse");
        assert_eq!(size.width, 1280);
        assert_eq!(size.height, 720);
    }

    #[test]
    fn test_size_parse_invalid_format() {
        assert!(Size::parse("1280-720").is_err());
        assert!(Size::parse("1280").is_err());
        assert!(Size::parse("axb").is_err());
    }

    #[test]
    fn test_size_display() {
        let size = Size::new(1920, 1080);
        assert_eq!(size.to_string(), "1920x1080");
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.device, "/dev/video10");
        assert_eq!(config.output_size.width, 1280);
        assert_eq!(config.fps, 30);
        assert_eq!(config.border_color, "#ff3333");
        assert_eq!(config.border_width, 4);
        assert_eq!(config.presets.len(), 3);
    }

    #[test]
    fn test_config_deserialize() {
        let yaml = "device: /dev/video5\noutput_size:\n  width: 1920\n  height: 1080\nfps: 60\nborder_color: \"#00ff00\"\nborder_width: 2\n";
        let config: Config = serde_yaml::from_str(yaml).expect("should deserialize");
        assert_eq!(config.device, "/dev/video5");
        assert_eq!(config.output_size.width, 1920);
        assert_eq!(config.fps, 60);
        assert_eq!(config.border_color, "#00ff00");
        assert_eq!(config.border_width, 2);
    }
}
