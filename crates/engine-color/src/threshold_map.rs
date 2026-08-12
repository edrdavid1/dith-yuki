//! Threshold map loading, validation, and sampling for ordered dithering.
//!
//! Provides:
//! - `ThresholdMap`: a loaded, normalized f32 threshold map for ordered dithering
//! - `ThresholdMapCache`: a concurrent cache (max 64 entries) keyed by (path, mtime)
//! - PNG loading with sandbox validation, grayscale enforcement, and dimension limits

use dashmap::DashMap;
use std::collections::VecDeque;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use thiserror::Error;

use engine_io::sandbox::{self, SandboxError};

/// A loaded threshold map: normalized f32 values in [0.0, 1.0], stored row-major.
#[derive(Debug)]
pub struct ThresholdMap {
    pub data: Vec<f32>,
    pub width: u32,
    pub height: u32,
}

impl ThresholdMap {
    /// Sample the map at global pixel coordinates (wraps via modulo).
    ///
    /// This enables seamless tiling of the threshold pattern across arbitrarily
    /// large images regardless of tile boundaries.
    pub fn sample(&self, global_x: u32, global_y: u32) -> f32 {
        let x = global_x % self.width;
        let y = global_y % self.height;
        self.data[(y * self.width + x) as usize]
    }
}

/// Errors that can occur when loading or validating a threshold map.
#[derive(Debug, Error)]
pub enum ThresholdMapError {
    #[error("not grayscale: found {actual} color type, expected 1-bit or 8-bit grayscale")]
    NotGrayscale { actual: String },

    #[error("dimensions {w}×{h} exceed maximum 4096×4096")]
    TooLarge { w: u32, h: u32 },

    #[error("I/O error: {0}")]
    Io(String),

    #[error("PNG decode error: {0}")]
    Decode(String),

    #[error("sandbox error: {0}")]
    Sandbox(#[from] SandboxError),
}

/// Cache key: canonical path + modification time.
type ThresholdCacheKey = (PathBuf, SystemTime);

/// Maximum number of cached threshold maps.
const MAX_CACHE_ENTRIES: usize = 64;

/// Global cache for loaded threshold maps (max 64 entries, LRU eviction).
pub struct ThresholdMapCache {
    entries: DashMap<ThresholdCacheKey, Arc<ThresholdMap>>,
    /// LRU order tracking: front = oldest, back = most recent.
    lru_order: Mutex<VecDeque<ThresholdCacheKey>>,
}

impl ThresholdMapCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
            lru_order: Mutex::new(VecDeque::new()),
        }
    }

    /// Load or retrieve a cached threshold map.
    ///
    /// Pipeline:
    /// 1. Validate path via sandbox (must be .png, within home dir)
    /// 2. Get file modification time
    /// 3. Check cache by (canonical_path, mtime)
    /// 4. On miss: read PNG, validate grayscale + dimensions, normalize, cache
    /// 5. LRU eviction at 64 entries
    pub fn get_or_load(&self, path: &Path) -> Result<Arc<ThresholdMap>, ThresholdMapError> {
        // 1. Resolve and validate the path via sandbox
        let canonical = sandbox::resolve_user_path(
            path.to_str().unwrap_or(""),
            &["png"],
        )?;

        // 2. Get file modification time
        let metadata = fs::metadata(&canonical)
            .map_err(|e| ThresholdMapError::Io(e.to_string()))?;
        let mtime = metadata
            .modified()
            .map_err(|e| ThresholdMapError::Io(e.to_string()))?;

        let key = (canonical.clone(), mtime);

        // 3. Check cache
        if let Some(entry) = self.entries.get(&key) {
            // Move to back of LRU
            if let Ok(mut lru) = self.lru_order.lock() {
                if let Some(pos) = lru.iter().position(|k| k == &key) {
                    lru.remove(pos);
                }
                lru.push_back(key.clone());
            }
            return Ok(Arc::clone(&entry));
        }

        // 4. Cache miss: load PNG from disk
        let bytes = fs::read(&canonical)
            .map_err(|e| ThresholdMapError::Io(e.to_string()))?;

        let threshold_map = load_png_threshold_map(&bytes)?;
        let arc_map = Arc::new(threshold_map);

        // 5. Evict if at capacity
        {
            let mut lru = self.lru_order.lock().unwrap_or_else(|e| e.into_inner());
            while lru.len() >= MAX_CACHE_ENTRIES {
                if let Some(oldest_key) = lru.pop_front() {
                    self.entries.remove(&oldest_key);
                }
            }
            lru.push_back(key.clone());
        }

        self.entries.insert(key, Arc::clone(&arc_map));
        Ok(arc_map)
    }
}

impl Default for ThresholdMapCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Load and validate a PNG file as a threshold map.
///
/// Validates:
/// - Color type must be Grayscale
/// - Bit depth must be 1 or 8
/// - Dimensions must be ≤ 4096×4096
///
/// Normalizes pixel values to [0.0, 1.0].
fn load_png_threshold_map(bytes: &[u8]) -> Result<ThresholdMap, ThresholdMapError> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|e| ThresholdMapError::Decode(e.to_string()))?;

    let info = reader.info();

    // Validate color type: must be Grayscale
    if info.color_type != png::ColorType::Grayscale {
        return Err(ThresholdMapError::NotGrayscale {
            actual: format!("{:?}", info.color_type),
        });
    }

    // Validate bit depth: must be 1 or 8
    match info.bit_depth {
        png::BitDepth::One | png::BitDepth::Eight => {}
        other => {
            return Err(ThresholdMapError::NotGrayscale {
                actual: format!("Grayscale {:?}-bit", other),
            });
        }
    }

    let width = info.width;
    let height = info.height;
    let bit_depth = info.bit_depth;

    // Validate dimensions
    if width > 4096 || height > 4096 {
        return Err(ThresholdMapError::TooLarge { w: width, h: height });
    }

    // Read all pixel data
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let output_info = reader
        .next_frame(&mut buf)
        .map_err(|e| ThresholdMapError::Decode(e.to_string()))?;
    buf.truncate(output_info.buffer_size());

    // Normalize to [0.0, 1.0]
    let data = match bit_depth {
        png::BitDepth::Eight => {
            buf.iter().map(|&v| v as f32 / 255.0).collect()
        }
        png::BitDepth::One => {
            // 1-bit: each byte contains 8 pixels (MSB first)
            let mut pixels = Vec::with_capacity((width * height) as usize);
            // Rows are byte-aligned in PNG
            let bytes_per_row = (width as usize + 7) / 8;
            for row in 0..height as usize {
                let row_start = row * bytes_per_row;
                for col in 0..width as usize {
                    let byte_idx = row_start + col / 8;
                    let bit_idx = 7 - (col % 8);
                    let bit = (buf[byte_idx] >> bit_idx) & 1;
                    pixels.push(bit as f32);
                }
            }
            pixels
        }
        _ => unreachable!(), // Already validated above
    };

    Ok(ThresholdMap { data, width, height })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a minimal valid 8-bit grayscale PNG in memory.
    fn create_grayscale_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut buf, width, height);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(pixels).unwrap();
        }
        buf
    }

    /// Helper: create a minimal 1-bit grayscale PNG in memory.
    fn create_1bit_grayscale_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut buf, width, height);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::One);
            let mut writer = encoder.write_header().unwrap();
            // pixels should be packed: each byte = 8 pixels, MSB first, rows byte-aligned
            writer.write_image_data(pixels).unwrap();
        }
        buf
    }

    /// Helper: create an RGB PNG (non-grayscale) in memory.
    fn create_rgb_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut buf, width, height);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(pixels).unwrap();
        }
        buf
    }

    #[test]
    fn test_sample_modulo_wrapping() {
        // 2×2 map with known values
        let map = ThresholdMap {
            data: vec![0.0, 0.25, 0.5, 0.75],
            width: 2,
            height: 2,
        };

        // Direct access
        assert_eq!(map.sample(0, 0), 0.0);
        assert_eq!(map.sample(1, 0), 0.25);
        assert_eq!(map.sample(0, 1), 0.5);
        assert_eq!(map.sample(1, 1), 0.75);

        // Modulo wrapping: x=2 wraps to 0, y=2 wraps to 0
        assert_eq!(map.sample(2, 0), 0.0);
        assert_eq!(map.sample(3, 0), 0.25);
        assert_eq!(map.sample(0, 2), 0.0);
        assert_eq!(map.sample(2, 2), 0.0);

        // Larger coordinates
        assert_eq!(map.sample(100, 100), 0.0); // 100%2=0, 100%2=0
        assert_eq!(map.sample(101, 101), 0.75); // 101%2=1, 101%2=1
        assert_eq!(map.sample(1000, 999), 0.5); // 1000%2=0, 999%2=1
    }

    #[test]
    fn test_sample_rectangular_map() {
        // 3×2 map
        let map = ThresholdMap {
            data: vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6],
            width: 3,
            height: 2,
        };

        assert_eq!(map.sample(0, 0), 0.1);
        assert_eq!(map.sample(1, 0), 0.2);
        assert_eq!(map.sample(2, 0), 0.3);
        assert_eq!(map.sample(0, 1), 0.4);
        assert_eq!(map.sample(1, 1), 0.5);
        assert_eq!(map.sample(2, 1), 0.6);

        // Wrapping
        assert_eq!(map.sample(3, 0), 0.1); // 3%3=0
        assert_eq!(map.sample(4, 0), 0.2); // 4%3=1
        assert_eq!(map.sample(0, 2), 0.1); // 2%2=0
    }

    #[test]
    fn test_load_8bit_grayscale_png() {
        let pixels = vec![0, 128, 255, 64];
        let png_bytes = create_grayscale_png(2, 2, &pixels);

        let map = load_png_threshold_map(&png_bytes).unwrap();
        assert_eq!(map.width, 2);
        assert_eq!(map.height, 2);
        assert_eq!(map.data.len(), 4);

        // Check normalization
        assert!((map.data[0] - 0.0).abs() < 1e-6);
        assert!((map.data[1] - 128.0 / 255.0).abs() < 1e-6);
        assert!((map.data[2] - 1.0).abs() < 1e-6);
        assert!((map.data[3] - 64.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn test_load_1bit_grayscale_png() {
        // 8×1 image: alternating black/white pixels
        // Packed as single byte: 0b10101010 = 0xAA
        let packed = vec![0xAA];
        let png_bytes = create_1bit_grayscale_png(8, 1, &packed);

        let map = load_png_threshold_map(&png_bytes).unwrap();
        assert_eq!(map.width, 8);
        assert_eq!(map.height, 1);
        assert_eq!(map.data.len(), 8);

        // MSB first: bit 7=1, bit 6=0, bit 5=1, bit 4=0, ...
        assert_eq!(map.data[0], 1.0);
        assert_eq!(map.data[1], 0.0);
        assert_eq!(map.data[2], 1.0);
        assert_eq!(map.data[3], 0.0);
        assert_eq!(map.data[4], 1.0);
        assert_eq!(map.data[5], 0.0);
        assert_eq!(map.data[6], 1.0);
        assert_eq!(map.data[7], 0.0);
    }

    #[test]
    fn test_reject_non_grayscale_png() {
        // Create an RGB PNG
        let pixels = vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 128, 128, 128];
        let png_bytes = create_rgb_png(2, 2, &pixels);

        let result = load_png_threshold_map(&png_bytes);
        assert!(result.is_err());
        match result.unwrap_err() {
            ThresholdMapError::NotGrayscale { actual } => {
                assert!(actual.contains("Rgb"), "Expected 'Rgb' in error, got: {}", actual);
            }
            other => panic!("Expected NotGrayscale error, got: {:?}", other),
        }
    }

    #[test]
    fn test_reject_oversized_dimensions() {
        // We can't easily create a 4097×4097 PNG in memory without huge allocation,
        // but we can test the validation logic directly by crafting a PNG header
        // that claims large dimensions. The png crate will read the header info
        // before allocating the full buffer.
        //
        // Instead, test via a 4097×1 image (only one dimension exceeds limit).
        // Creating 4097 pixels for a grayscale 8-bit image:
        let pixels = vec![128u8; 4097];
        let png_bytes = create_grayscale_png(4097, 1, &pixels);

        let result = load_png_threshold_map(&png_bytes);
        assert!(result.is_err());
        match result.unwrap_err() {
            ThresholdMapError::TooLarge { w, h } => {
                assert_eq!(w, 4097);
                assert_eq!(h, 1);
            }
            other => panic!("Expected TooLarge error, got: {:?}", other),
        }
    }

    #[test]
    fn test_reject_oversized_height() {
        let pixels = vec![128u8; 4097];
        let png_bytes = create_grayscale_png(1, 4097, &pixels);

        let result = load_png_threshold_map(&png_bytes);
        assert!(result.is_err());
        match result.unwrap_err() {
            ThresholdMapError::TooLarge { w, h } => {
                assert_eq!(w, 1);
                assert_eq!(h, 4097);
            }
            other => panic!("Expected TooLarge error, got: {:?}", other),
        }
    }

    #[test]
    fn test_valid_max_dimensions_accepted() {
        // 4096×1 should be accepted
        let pixels = vec![200u8; 4096];
        let png_bytes = create_grayscale_png(4096, 1, &pixels);

        let result = load_png_threshold_map(&png_bytes);
        assert!(result.is_ok());
        let map = result.unwrap();
        assert_eq!(map.width, 4096);
        assert_eq!(map.height, 1);
    }

    #[test]
    fn test_invalid_png_bytes_returns_decode_error() {
        let garbage = vec![0u8, 1, 2, 3, 4, 5];
        let result = load_png_threshold_map(&garbage);
        assert!(result.is_err());
        match result.unwrap_err() {
            ThresholdMapError::Decode(_) => {} // expected
            other => panic!("Expected Decode error, got: {:?}", other),
        }
    }

    #[test]
    fn test_threshold_map_cache_new() {
        let cache = ThresholdMapCache::new();
        assert_eq!(cache.entries.len(), 0);
    }
}
