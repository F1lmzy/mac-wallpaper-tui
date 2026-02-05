use anyhow::Result;
use image::{GenericImageView, ImageBuffer, ImageReader, Rgba};
use ratatui::layout::Rect;
use ratatui_image::{picker::Picker, protocol::StatefulProtocol};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio::time::timeout;

const PREVIEW_WIDTH: u32 = 100; // Lower resolution for faster loading
const PREVIEW_HEIGHT: u32 = 75; // Lower resolution for faster loading
const ENABLE_BLUR: bool = false; // Disable blur for maximum speed
const THUMBNAIL_TIMEOUT_MS: u64 = 500; // Increased timeout for initial thumbnail generation

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
                    if let Ok(Some(info)) =
                        Self::ensure_thumbnail_with_defaults(&path, thumb_dir).await
                    {
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

    // Apply Gaussian blur to smooth out pixelation from low-res images
    // Optimized implementation using box blur approximation for speed
    fn apply_gaussian_blur(
        img: ImageBuffer<Rgba<u8>, Vec<u8>>,
        sigma: f32,
    ) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        let (width, height) = img.dimensions();

        // For very small sigma, skip blur to save time
        if sigma < 0.5 {
            return img;
        }

        // Use smaller kernel for faster processing
        let kernel_size = ((sigma * 2.0).ceil() as i32).min(3); // Cap at 3 for speed

        if kernel_size < 1 {
            return img;
        }

        // Pre-calculate Gaussian weights
        let mut weights = Vec::new();
        let mut weight_sum = 0.0;
        for i in -kernel_size..=kernel_size {
            let weight = (-((i * i) as f32) / (2.0 * sigma * sigma)).exp();
            weights.push(weight);
            weight_sum += weight;
        }
        // Normalize weights
        for w in &mut weights {
            *w /= weight_sum;
        }

        // Horizontal pass
        let mut temp = ImageBuffer::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let mut r = 0.0;
                let mut g = 0.0;
                let mut b = 0.0;
                let mut a = 0.0;

                for (i, &weight) in weights.iter().enumerate() {
                    let offset = i as i32 - kernel_size;
                    let sample_x = (x as i32 + offset).clamp(0, width as i32 - 1) as u32;
                    let pixel = img.get_pixel(sample_x, y);

                    r += pixel[0] as f32 * weight;
                    g += pixel[1] as f32 * weight;
                    b += pixel[2] as f32 * weight;
                    a += pixel[3] as f32 * weight;
                }

                temp.put_pixel(
                    x,
                    y,
                    Rgba([
                        r.round() as u8,
                        g.round() as u8,
                        b.round() as u8,
                        a.round() as u8,
                    ]),
                );
            }
        }

        // Vertical pass
        let mut result = ImageBuffer::new(width, height);
        for x in 0..width {
            for y in 0..height {
                let mut r = 0.0;
                let mut g = 0.0;
                let mut b = 0.0;
                let mut a = 0.0;

                for (i, &weight) in weights.iter().enumerate() {
                    let offset = i as i32 - kernel_size;
                    let sample_y = (y as i32 + offset).clamp(0, height as i32 - 1) as u32;
                    let pixel = temp.get_pixel(x, sample_y);

                    r += pixel[0] as f32 * weight;
                    g += pixel[1] as f32 * weight;
                    b += pixel[2] as f32 * weight;
                    a += pixel[3] as f32 * weight;
                }

                result.put_pixel(
                    x,
                    y,
                    Rgba([
                        r.round() as u8,
                        g.round() as u8,
                        b.round() as u8,
                        a.round() as u8,
                    ]),
                );
            }
        }

        result
    }

    pub fn preload(&self, path: PathBuf) {
        let _ = self.preload_tx.send(path);
    }

    pub fn preload_all(&self, paths: Vec<PathBuf>) {
        for path in paths {
            let _ = self.preload_tx.send(path);
        }
    }

    pub async fn get_image(
        &self,
        path: &Path,
        target_rect: Option<Rect>,
    ) -> Result<Option<ImageLoadResult>> {
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
                // Generate thumbnail synchronously but with timeout
                match timeout(
                    Duration::from_millis(THUMBNAIL_TIMEOUT_MS),
                    Self::ensure_thumbnail(
                        path,
                        self.thumbnail_dir.clone(),
                        target_rect,
                        self.picker.clone(),
                    ),
                )
                .await
                {
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
        let result =
            Self::load_from_thumbnail(&thumbnail_path, self.picker.clone(), target_rect).await;

        let elapsed = start.elapsed();
        if elapsed > Duration::from_millis(100) {
            eprintln!("Slow image load: {:?} took {:?}", path, elapsed);
        }

        result
    }

    async fn ensure_thumbnail(
        path: &Path,
        thumbnail_dir: PathBuf,
        _target_rect: Option<Rect>,
        _picker: Picker,
    ) -> Result<Option<ImageInfo>> {
        // Always use fixed preview dimensions to avoid double-resizing
        Self::ensure_thumbnail_internal(path, thumbnail_dir, PREVIEW_WIDTH, PREVIEW_HEIGHT).await
    }

    // Helper for background preloading - uses default dimensions (no Rect/picker needed)
    async fn ensure_thumbnail_with_defaults(
        path: &Path,
        thumbnail_dir: PathBuf,
    ) -> Result<Option<ImageInfo>> {
        // For background preloading, use fixed preview dimensions
        Self::ensure_thumbnail_internal(path, thumbnail_dir, PREVIEW_WIDTH, PREVIEW_HEIGHT).await
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
            })
            .await;

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
                // Use sips with exact target dimensions for faster conversion
                let max_dimension = default_width.max(default_height);
                let output = std::process::Command::new("sips")
                    .args(&[
                        "-s",
                        "format",
                        "jpeg",
                        "-s",
                        "formatOptions",
                        "70", // Slightly higher quality
                        "-Z",
                        &max_dimension.to_string(),
                        source_path.to_str().unwrap(),
                        "--out",
                        thumb_path.to_str().unwrap(),
                    ])
                    .output()?;

                if !output.status.success() {
                    return Err(anyhow::anyhow!(
                        "sips failed: {:?}",
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }

                let reader = ImageReader::open(&thumb_path)?;
                let image = reader.decode()?;
                Ok((image.dimensions().0, image.dimensions().1))
            } else {
                let mut reader = ImageReader::open(&source_path)?;
                reader.no_limits(); // Allow large images but we'll thumbnail them

                let image = reader.decode()?;
                let dims = image.dimensions();

                // Use thumbnail() method for fast downsampling - much faster than resize()
                let thumbnail = image.thumbnail(default_width, default_height);

                // Save directly without blur for maximum speed
                thumbnail.save_with_format(&thumb_path, image::ImageFormat::Jpeg)?;

                Ok(dims)
            };

            let elapsed = start.elapsed();
            if elapsed > Duration::from_millis(100) {
                eprintln!(
                    "Slow thumbnail generation: {:?} took {:?}",
                    source_path, elapsed
                );
            }

            result
        })
        .await;

        match gen_result {
            Ok(Ok((width, height))) => Ok(Some(ImageInfo {
                thumbnail_path,
                dimensions: (width, height),
            })),
            _ => Ok(None),
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
            let thumbnail = reader.decode()?;
            let original_dimensions = thumbnail.dimensions();

            // Scale thumbnail to fill the preview window
            let final_image = if let Some(rect) = target_rect {
                let (font_width, font_height) = picker.font_size();
                let target_width = (rect.width as f32 * font_width as f32) as u32;
                let target_height = (rect.height as f32 * font_height as f32) as u32;

                // Only scale up if the target is significantly larger than thumbnail
                if target_width > thumbnail.dimensions().0
                    || target_height > thumbnail.dimensions().1
                {
                    // Use Nearest for fast upscaling from our small thumbnail
                    thumbnail.resize(
                        target_width,
                        target_height,
                        image::imageops::FilterType::Nearest,
                    )
                } else {
                    thumbnail
                }
            } else {
                thumbnail
            };

            let protocol = picker.new_resize_protocol(final_image);

            let elapsed = start.elapsed();
            if elapsed > Duration::from_millis(50) {
                eprintln!(
                    "Thumbnail load: {:?} ({}x{}) took {:?}",
                    path_buf.file_name().unwrap_or_default(),
                    original_dimensions.0,
                    original_dimensions.1,
                    elapsed
                );
            }

            Ok::<_, anyhow::Error>((protocol, original_dimensions))
        })
        .await;

        match result {
            Ok(Ok((protocol, dimensions))) => Ok(Some(ImageLoadResult {
                protocol,
                dimensions,
            })),
            _ => Ok(None),
        }
    }

    fn get_thumbnail_path(&self, original_path: &Path) -> PathBuf {
        self.thumbnail_dir
            .join(format!("{}.jpg", Self::hash_path(original_path)))
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
