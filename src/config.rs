use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_root_directory")]
    pub root_directory: PathBuf,

    #[serde(default)]
    pub show_hidden: bool,

    #[serde(default = "default_preview_size")]
    pub preview_size: u32,
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;

        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: Config = toml::from_str(&content)?;
            Ok(config)
        } else {
            let config = Self::default();
            config.save()?;
            Ok(config)
        }
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;
        std::fs::create_dir_all(config_path.parent().unwrap())?;

        let content = toml::to_string_pretty(self)?;
        std::fs::write(&config_path, content)?;

        Ok(())
    }

    fn config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("mac-wallpaper-tui");

        Ok(config_dir.join("config.toml"))
    }

    pub fn is_valid_image(&self, path: &Path) -> bool {
        if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            // Note: madesktop files are macOS dynamic wallpapers,
            // but we can't preview them - only set them as wallpapers
            ["jpg", "jpeg", "png", "heic", "webp"].contains(&ext.as_str())
        } else {
            false
        }
    }

    pub fn is_valid_wallpaper(&self, path: &Path) -> bool {
        // For setting wallpapers, also accept .madesktop files
        if self.is_valid_image(path) {
            return true;
        }
        if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            ext == "madesktop"
        } else {
            false
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            root_directory: default_root_directory(),
            show_hidden: false,
            preview_size: default_preview_size(),
        }
    }
}

fn default_root_directory() -> PathBuf {
    // Try system wallpapers first
    let system_wallpapers = PathBuf::from("/System/Library/Desktop Pictures");
    if system_wallpapers.exists() {
        return system_wallpapers;
    }

    // Fall back to user Pictures
    dirs::home_dir()
        .map(|h| h.join("Pictures"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| {
            // Last resort: current directory
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        })
}

fn default_preview_size() -> u32 {
    400
}
