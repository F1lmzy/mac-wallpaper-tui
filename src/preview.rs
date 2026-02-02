use anyhow::Result;
use image::{ImageReader, imageops::FilterType, GenericImageView};
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use ratatui::layout::Rect;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, mpsc};
use tokio::time::timeout;

const PREVIEW_WIDTH: u32 = 400;  // Fixed preview width for speed
const PREVIEW_HEIGHT: u32 = 300; // Fixed preview height for speed

pub struct ImageCache {
    cache: Arc<RwLock<HashMap<PathBuf, ImageInfo>>>,
    picker: Picker,
    thumbnail_dir: PathBuf,
    preload_tx: mpsc::UnboundedSender<PathBuf>,
}

#[derive(Clone)]
pub struct ImageInfo {
    pub thumbnail_path: PathBuf,
    pub dimensions: (u32, u32),
}

#[derive(Clone)]
pub struct CachedImage {
    pub dimensions: (u32, u32),
    pub size_bytes: u64,
}

pub struct ImageLoadResult {
    pub protocol: StatefulProtocol,
    pub dimensions: (u32, u32),
}

impl ImageCache {
    pub fn new() -> Result<Self> {
        let picker = Picker::from_query_stdio()?;
        
        let thumbnail_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from(".cache"))
            .join("mac-wallpaper-tui")
            .join("thumbnails");
        
        std::fs::create_dir_all(&thumbnail_dir)?;
        
        let cache = Arc::new(RwLock::new(HashMap::new()));
        let (preload_tx, mut preload_rx) = mpsc::unbounded_channel::<PathBuf>();
        
        // Spawn background preloader thread
        let cache_clone = Arc::clone(&cache);
        let thumbnail_dir_clone = thumbnail_dir.clone();
        
        tokio::spawn(async move {
            while let Some(path) = preload_rx.recv().await {
                let cache = Arc::clone(&cache_clone);
                let thumb_dir = thumbnail_dir_clone.clone();
                
                // Spawn blocking task for each preload
                // Note: Background preloading uses default dimensions
                tokio::task::spawn(async move {
                    if let Ok(Some(info)) = Self::ensure_thumbnail_with_defaults(&path, thumb_dir).await {
                        let mut cache_guard = cache.write().await;
                        cache_guard.insert(path, info);
                    }
                });
            }
        });
        
        Ok(Self {
            cache,
            picker,
            thumbnail_dir,
            preload_tx,
        })
    }

    pub fn preload(&self, path: PathBuf) {
        let _ = self.preload_tx.send(path);
    }

    pub fn preload_all(&self, paths: Vec<PathBuf>) {
        for path in paths {
            let _ = self.preload_tx.send(path);
        }
    }

    pub async fn get_image(&self, path: &Path, target_rect: Option<Rect>) -> Result<Option<ImageLoadResult>> {
        let start = Instant::now();
        
        // Check memory cache for thumbnail path
        let cache = self.cache.read().await;
        let cached_info = cache.get(path).cloned();
        drop(cache);
        
        let thumbnail_path = if let Some(info) = cached_info {
            info.thumbnail_path
        } else {
            // Fast path: check if thumbnail exists without generating
            let thumb_path = self.get_thumbnail_path(path);
            if thumb_path.exists() {
                thumb_path
            } else {
                // Generate thumbnail synchronously but with short timeout
                match timeout(Duration::from_millis(200), Self::ensure_thumbnail(path, self.thumbnail_dir.clone(), target_rect, self.picker.clone())).await {
                    Ok(Ok(Some(info))) => {
                        // Cache the info
                        let mut cache = self.cache.write().await;
                        cache.insert(path.to_path_buf(), info.clone());
                        info.thumbnail_path
                    }
                    _ => {
                        // Timeout or error - trigger background generation
                        self.preload(path.to_path_buf());
                        return Ok(None);
                    }
                }
            }
        };

        // Load from thumbnail (this is fast - just decoding a small JPEG)
        let result = Self::load_from_thumbnail(&thumbnail_path, self.picker.clone(), target_rect).await;
        
        let elapsed = start.elapsed();
        if elapsed > Duration::from_millis(100) {
            eprintln!("Slow image load: {:?} took {:?}", path, elapsed);
        }
        
        result
    }

    async fn ensure_thumbnail(
        path: &Path,
        thumbnail_dir: PathBuf,
        target_rect: Option<Rect>,
        picker: Picker,
    ) -> Result<Option<ImageInfo>> {
        // Calculate dimensions based on target_rect or use defaults
        let (target_width, target_height) = if let Some(rect) = target_rect {
            let (font_width, font_height) = picker.font_size();
            let pixel_width = (rect.width as f32 * font_width as f32) as u32;
            let pixel_height = (rect.height as f32 * font_height as f32) as u32;
            (pixel_width.min(800), pixel_height.min(600)) // Cap max thumbnail size
        } else {
            (400, 300)
        };
        
        Self::ensure_thumbnail_internal(path, thumbnail_dir, target_width, target_height).await
    }

    // Helper for background preloading - uses default dimensions (no Rect/picker needed)
    async fn ensure_thumbnail_with_defaults(
        path: &Path,
        thumbnail_dir: PathBuf,
    ) -> Result<Option<ImageInfo>> {
        // For background preloading, we use default dimensions (400x300)
        // This is equivalent to calling ensure_thumbnail with None for target_rect
        Self::ensure_thumbnail_internal(path, thumbnail_dir, 400, 300).await
    }
    
    // Internal implementation that takes raw pixel dimensions
    async fn ensure_thumbnail_internal(
        path: &Path,
        thumbnail_dir: PathBuf,
        default_width: u32,
        default_height: u32,
    ) -> Result<Option<ImageInfo>> {
        if !Self::is_image_file(path) {
            return Ok(None);
        }

        let thumbnail_path = thumbnail_dir.join(format!("{}.jpg", Self::hash_path(path)));
        
        if thumbnail_path.exists() {
            let thumb_path = thumbnail_path.clone();
            let dimensions = tokio::task::spawn_blocking(move || {
                let reader = ImageReader::open(&thumb_path)?;
                let image = reader.decode()?;
                Ok::<_, anyhow::Error>(image.dimensions())
            }).await;
            
            if let Ok(Ok(dims)) = dimensions {
                return Ok(Some(ImageInfo {
                    thumbnail_path,
                    dimensions: dims,
                }));
            }
        }

        let is_heic = Self::is_heic_file(path);
        let source_path = path.to_path_buf();
        let thumb_path = thumbnail_path.clone();
        
        let gen_result = tokio::task::spawn_blocking(move || {
            let start = Instant::now();
            
            let result = if is_heic {
                let output = std::process::Command::new("sips")
                    .args(&[
                        "-s", "format", "jpeg",
                        "-s", "formatOptions", "60",
                        "-Z", "400",
                        source_path.to_str().unwrap(),
                        "--out", thumb_path.to_str().unwrap()
                    ])
                    .output()?;
                
                if !output.status.success() {
                    return Err(anyhow::anyhow!("sips failed"));
                }
                
                let reader = ImageReader::open(&thumb_path)?;
                let image = reader.decode()?;
                Ok((image.dimensions().0, image.dimensions().1))
            } else {
                let reader = ImageReader::open(&source_path)?;
                let image = reader.decode()?;
                let dims = image.dimensions();
                
                let small = image.resize(default_width, default_height, FilterType::Nearest);
                small.save(&thumb_path)?;
                
                Ok(dims)
            };
            
            let elapsed = start.elapsed();
            if elapsed > Duration::from_millis(100) {
                eprintln!("Slow thumbnail generation: {:?} took {:?}", source_path, elapsed);
            }
            
            result
        }).await;
        
        match gen_result {
            Ok(Ok((width, height))) => {
                Ok(Some(ImageInfo {
                    thumbnail_path,
                    dimensions: (width, height),
                }))
            }
            _ => Ok(None)
        }
    }

    async fn load_from_thumbnail(
        thumbnail_path: &Path,
        picker: Picker,
        target_rect: Option<Rect>,
    ) -> Result<Option<ImageLoadResult>> {
        let path_buf = thumbnail_path.to_path_buf();
        
        let result = tokio::task::spawn_blocking(move || {
            let start = Instant::now();
            
            let reader = ImageReader::open(&path_buf)?;
            let image = reader.decode()?;
            let dimensions = image.dimensions();
            
            // Calculate target dimensions based on provided rect or use defaults
            let (target_width, target_height) = if let Some(rect) = target_rect {
                let (font_width, font_height) = picker.font_size();
                let pixel_width = (rect.width as f32 * font_width as f32) as u32;
                let pixel_height = (rect.height as f32 * font_height as f32) as u32;
                (pixel_width, pixel_height)
            } else {
                (PREVIEW_WIDTH, PREVIEW_HEIGHT)
            };
            
            // Fast resize for preview - use Nearest for speed
            let image = image.resize(target_width, target_height, FilterType::Nearest);
            let protocol = picker.new_resize_protocol(image);
            
            let elapsed = start.elapsed();
            if elapsed > Duration::from_millis(50) {
                eprintln!("Slow thumbnail decode: {:?} took {:?}", path_buf, elapsed);
            }
            
            Ok::<_, anyhow::Error>((protocol, dimensions))
        }).await;

        match result {
            Ok(Ok((protocol, dimensions))) => Ok(Some(ImageLoadResult { protocol, dimensions })),
            _ => Ok(None),
        }
    }

    fn get_thumbnail_path(&self, original_path: &Path) -> PathBuf {
        self.thumbnail_dir.join(format!("{}.jpg", Self::hash_path(original_path)))
    }

    fn hash_path(path: &Path) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        path.to_string_lossy().hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    fn is_image_file(path: &Path) -> bool {
        if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "heic" | "webp")
        } else {
            false
        }
    }

    fn is_heic_file(path: &Path) -> bool {
        if let Some(ext) = path.extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            ext == "heic"
        } else {
            false
        }
    }

    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}

impl Clone for ImageCache {
    fn clone(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
            picker: self.picker.clone(),
            thumbnail_dir: self.thumbnail_dir.clone(),
            preload_tx: self.preload_tx.clone(),
        }
    }
}
