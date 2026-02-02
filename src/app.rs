use anyhow::Result;
use std::path::PathBuf;
use ratatui_image::protocol::StatefulProtocol;
use rand::seq::SliceRandom;
use crate::preview::{CachedImage, ImageCache};
use crate::database::Database;
use crate::config::Config;

use ratatui::layout::Rect;

pub struct App {
    pub root_dir: PathBuf,
    pub current_dir: PathBuf,
    pub items: Vec<PathBuf>,
    pub selected_index: usize,
    pub image_cache: ImageCache,
    pub current_preview: Option<CachedImage>,
    pub current_protocol: Option<StatefulProtocol>,
    pub status_message: Option<String>,
    pub favorites: Vec<PathBuf>,
    pub recent_wallpapers: Vec<PathBuf>,
    pub show_recent: bool,
    pub database: Database,
    pub config: Config,
    pub preview_area: Option<Rect>,
}

impl App {
    pub async fn new() -> Result<Self> {
        let config = Config::load()?;
        let root_dir = config.root_directory.clone();
        let current_dir = root_dir.clone();
        let items = Self::list_directory(&current_dir, &root_dir, &config);
        let image_cache = ImageCache::new()?;
        let database = Database::new()?;
        let favorites = database.get_favorites()?;
        let recent_wallpapers = database.get_recent_wallpapers(10)?;
        
        let mut app = Self {
            root_dir,
            current_dir,
            items,
            selected_index: 0,
            image_cache,
            current_preview: None,
            current_protocol: None,
            status_message: None,
            favorites,
            recent_wallpapers,
            show_recent: false,
            database,
            config,
            preview_area: None,
        };
        
        // Load initial preview
        app.update_preview().await;
        
        Ok(app)
    }

    fn list_directory(path: &PathBuf, root_dir: &PathBuf, config: &Config) -> Vec<PathBuf> {
        let mut items = vec![];
        
        // Only add ".." if we're not at the root directory
        if path != root_dir {
            items.push(PathBuf::from(".."));
        }
        
        if let Ok(entries) = std::fs::read_dir(path) {
            let mut dirs = vec![];
            let mut files = vec![];
            
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else {
                    // Only include image files
                    if config.is_valid_wallpaper(&path) {
                        files.push(path);
                    }
                }
            }
            
            // Sort directories first, then files
            dirs.sort();
            files.sort();
            
            items.extend(dirs);
            items.extend(files);
        }
        
        items
    }

    pub async fn next(&mut self) {
        if !self.items.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.items.len();
            self.update_preview().await;
        }
    }

    pub async fn previous(&mut self) {
        if !self.items.is_empty() {
            if self.selected_index == 0 {
                self.selected_index = self.items.len() - 1;
            } else {
                self.selected_index -= 1;
            }
            self.update_preview().await;
        }
    }

    pub async fn enter(&mut self) {
        if self.show_recent {
            // Select from recent wallpapers
            if let Some(path) = self.recent_wallpapers.get(self.selected_index).cloned() {
                self.set_wallpaper_from_path(&path).await.ok();
            }
            self.show_recent = false;
            self.items = Self::list_directory(&self.current_dir, &self.root_dir, &self.config);
            // Preload all images in the directory
            let image_paths: Vec<PathBuf> = self.items.iter()
                .filter(|p| self.config.is_valid_image(p))
                .cloned()
                .collect();
            self.image_cache.preload_all(image_paths);
        } else if let Some(selected) = self.items.get(self.selected_index) {
            let new_path = if selected.file_name() == Some(std::ffi::OsStr::new("..")) {
                // Only allow going up if parent is still within root
                self.current_dir.parent().filter(|p| p.starts_with(&self.root_dir)).map(|p| p.to_path_buf())
            } else {
                Some(selected.clone())
            };
            
            if let Some(new_path) = new_path {
                if new_path.is_dir() {
                    self.current_dir = new_path;
                    self.items = Self::list_directory(&self.current_dir, &self.root_dir, &self.config);
                    // Preload all images in the directory
                    let image_paths: Vec<PathBuf> = self.items.iter()
                        .filter(|p| self.config.is_valid_image(p))
                        .cloned()
                        .collect();
                    self.image_cache.preload_all(image_paths);
                    self.selected_index = 0;
                    self.update_preview().await;
                }
            }
        }
    }

    pub async fn go_back(&mut self) {
        if self.show_recent {
            self.show_recent = false;
            self.items = Self::list_directory(&self.current_dir, &self.root_dir, &self.config);
            // Preload all images in the directory
            let image_paths: Vec<PathBuf> = self.items.iter()
                .filter(|p| self.config.is_valid_image(p))
                .cloned()
                .collect();
            self.image_cache.preload_all(image_paths);
            self.selected_index = 0;
            self.update_preview().await;
        } else if let Some(parent) = self.current_dir.parent() {
            // Only go back if parent is still within or equal to root
            if parent.starts_with(&self.root_dir) {
                self.current_dir = parent.to_path_buf();
                self.items = Self::list_directory(&self.current_dir, &self.root_dir, &self.config);
                // Preload all images in the directory
                let image_paths: Vec<PathBuf> = self.items.iter()
                    .filter(|p| self.config.is_valid_image(p))
                    .cloned()
                    .collect();
                self.image_cache.preload_all(image_paths);
                self.selected_index = 0;
                self.update_preview().await;
            }
        }
    }

    async fn update_preview(&mut self) {
        self.current_preview = None;
        self.current_protocol = None;
        
        let selected = if self.show_recent {
            self.recent_wallpapers.get(self.selected_index)
        } else {
            self.items.get(self.selected_index)
        };
        
        if let Some(path) = selected {
            if path.is_file() {
                // Fast path: load image immediately (uses thumbnail cache internally)
                match self.image_cache.get_image(path, self.preview_area).await {
                    Ok(Some(result)) => {
                        // Store dimensions for display
                        self.current_preview = Some(CachedImage {
                            dimensions: result.dimensions,
                            size_bytes: 0,
                        });
                        self.current_protocol = Some(result.protocol);
                    }
                    Ok(None) => {
                        // Trigger background preload for next time
                        self.image_cache.preload(path.clone());
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error loading image: {}", e));
                    }
                }
            }
        }
    }

    pub fn toggle_favorite(&mut self) {
        let selected = if self.show_recent {
            self.recent_wallpapers.get(self.selected_index)
        } else {
            self.items.get(self.selected_index)
        };
        
        if let Some(path) = selected {
            if path.is_file() {
                let path = path.clone();
                if self.is_favorite(&path) {
                    if let Err(e) = self.database.remove_favorite(&path) {
                        self.status_message = Some(format!("Error: {}", e));
                        return;
                    }
                    self.favorites.retain(|p| p != &path);
                    self.status_message = Some("Removed from favorites".to_string());
                } else {
                    if let Err(e) = self.database.add_favorite(&path) {
                        self.status_message = Some(format!("Error: {}", e));
                        return;
                    }
                    self.favorites.push(path);
                    self.status_message = Some("Added to favorites".to_string());
                }
            }
        }
    }

    pub fn is_favorite(&self, path: &PathBuf) -> bool {
        self.favorites.contains(path)
    }

    pub fn show_recent_wallpapers(&mut self) {
        if self.show_recent {
            self.show_recent = false;
            self.items = Self::list_directory(&self.current_dir, &self.root_dir, &self.config);
            // Preload all images in the directory
            let image_paths: Vec<PathBuf> = self.items.iter()
                .filter(|p| self.config.is_valid_image(p))
                .cloned()
                .collect();
            self.image_cache.preload_all(image_paths);
        } else {
            self.show_recent = true;
            self.items = self.recent_wallpapers.clone();
            if self.items.is_empty() {
                self.status_message = Some("No recent wallpapers".to_string());
                self.show_recent = false;
                self.items = Self::list_directory(&self.current_dir, &self.root_dir, &self.config);
                // Preload all images in the directory
                let image_paths: Vec<PathBuf> = self.items.iter()
                    .filter(|p| self.config.is_valid_image(p))
                    .cloned()
                    .collect();
                self.image_cache.preload_all(image_paths);
            }
        }
        self.selected_index = 0;
    }

    pub async fn set_random_wallpaper(&mut self) -> Result<()> {
        // Collect all wallpaper files from current directory and subdirectories
        let mut all_images = vec![];
        self.collect_images(&self.current_dir, &mut all_images)?;
        
        if all_images.is_empty() {
            self.status_message = Some("No images found".to_string());
            return Ok(());
        }
        
        // Select random image
        let mut rng = rand::thread_rng();
        if let Some(path) = all_images.choose(&mut rng) {
            self.set_wallpaper_from_path(path).await?;
        }
        
        Ok(())
    }

    fn collect_images(&self, dir: &PathBuf, images: &mut Vec<PathBuf>) -> Result<()> {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Recursively search subdirectories (limit depth to avoid performance issues)
                    if images.len() < 1000 {
                        self.collect_images(&path, images)?;
                    }
                } else if self.config.is_valid_image(&path) {
                    images.push(path);
                }
            }
        }
        Ok(())
    }

    pub async fn set_wallpaper(&mut self) -> Result<()> {
        let path = if self.show_recent {
            self.recent_wallpapers.get(self.selected_index).cloned()
        } else {
            self.items.get(self.selected_index).cloned()
        };
        
        if let Some(path) = path {
            if path.is_file() {
                self.set_wallpaper_from_path(&path).await?;
            }
        }
        Ok(())
    }

    async fn set_wallpaper_from_path(&mut self, path: &PathBuf) -> Result<()> {
        let path_str = path.to_string_lossy();
        let script = format!(
            r#"tell application "System Events" to tell every desktop to set picture to "{}""#,
            path_str
        );
        
        let output = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()?;
        
        if output.status.success() {
            let name = path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string_lossy().to_string());
            self.status_message = Some(format!("Set wallpaper: {}", name));
            
            // Add to recent wallpapers
            if let Err(e) = self.database.add_recent_wallpaper(path) {
                eprintln!("Failed to add to recent: {}", e);
            }
            self.recent_wallpapers = self.database.get_recent_wallpapers(10)?;
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            self.status_message = Some(format!("Failed: {}", stderr));
            return Err(anyhow::anyhow!("Failed to set wallpaper: {}", stderr));
        }
        
        Ok(())
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    pub fn on_tick(&mut self) {
        // Clear status messages after a few seconds (placeholder)
    }

    pub fn selected_item(&self) -> Option<&PathBuf> {
        if self.show_recent {
            self.recent_wallpapers.get(self.selected_index)
        } else {
            self.items.get(self.selected_index)
        }
    }
}
