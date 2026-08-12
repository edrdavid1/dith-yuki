//! Palette generation algorithms (MedianCut, KMeans).
//!
//! Generates a reduced palette from a set of input pixels using
//! color quantization methods.

use std::collections::HashSet;

use super::{LinearColor, PaletteError};

/// Cap samples fed into MedianCut / K-Means so large photos stay interactive.
/// Stride-sampling in the Tauri command should keep counts near this already;
/// this is a second safety net.
pub const MAX_GENERATION_SAMPLES: usize = 200_000;

/// Available palette generation methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteGenMethod {
    /// Median-cut algorithm: recursively splits bounding boxes along the longest axis.
    MedianCut,
    /// K-Means clustering with k-means++ initialization.
    KMeans,
}

/// Generate a palette from an iterator of linear RGB pixels.
///
/// Skips fully transparent pixels (note: LinearColor has no alpha, so the
/// caller is responsible for filtering transparent pixels before passing).
///
/// # Arguments
/// * `pixels` - Iterator of linear RGB pixels (pre-filtered for transparency)
/// * `target_count` - Desired number of palette colors (2–256)
/// * `method` - Algorithm to use for generation
///
/// # Errors
/// Returns `PaletteError::GenerationFailed` if the input is empty.
pub fn generate_palette(
    pixels: impl Iterator<Item = LinearColor>,
    target_count: u16,
    method: PaletteGenMethod,
) -> Result<Vec<LinearColor>, PaletteError> {
    let mut pixel_list: Vec<LinearColor> = pixels.collect();

    if pixel_list.is_empty() {
        return Err(PaletteError::GenerationFailed(
            "no pixels provided for palette generation".to_string(),
        ));
    }

    pixel_list = subsample_pixels(pixel_list, MAX_GENERATION_SAMPLES);

    // Deduplicate: if fewer unique colors than target, return only unique colors
    let unique = deduplicate(&pixel_list);
    if unique.len() <= target_count as usize {
        return Ok(unique);
    }

    match method {
        PaletteGenMethod::MedianCut => median_cut(pixel_list, target_count as usize),
        PaletteGenMethod::KMeans => kmeans(pixel_list, target_count as usize),
    }
}

/// Evenly thin a pixel list down to at most `max_samples`.
pub fn subsample_pixels(pixels: Vec<LinearColor>, max_samples: usize) -> Vec<LinearColor> {
    if max_samples == 0 || pixels.len() <= max_samples {
        return pixels;
    }
    let stride = pixels.len().div_ceil(max_samples).max(1);
    pixels.into_iter().step_by(stride).collect()
}

/// Pack quantized linear RGB into a u32 key for O(1) uniqueness checks.
#[inline]
fn color_key(c: &LinearColor) -> u32 {
    let r = (c.r.clamp(0.0, 1.0) * 255.0).round() as u32;
    let g = (c.g.clamp(0.0, 1.0) * 255.0).round() as u32;
    let b = (c.b.clamp(0.0, 1.0) * 255.0).round() as u32;
    (r << 16) | (g << 8) | b
}

/// Deduplicate colors via HashSet (O(n)), not pairwise scan (O(n²)).
fn deduplicate(pixels: &[LinearColor]) -> Vec<LinearColor> {
    let mut seen = HashSet::with_capacity(pixels.len().min(65_536));
    let mut unique: Vec<LinearColor> = Vec::new();
    for &p in pixels {
        if seen.insert(color_key(&p)) {
            unique.push(p);
        }
    }
    unique
}

// =============================================================================
// Median Cut
// =============================================================================

/// Median-cut palette generation.
///
/// 1. Find the bounding box of all colors in RGB space
/// 2. Split along the longest axis at the median
/// 3. Recurse until target_count bins are reached
/// 4. Return the mean color of each bin
fn median_cut(
    pixels: Vec<LinearColor>,
    target_count: usize,
) -> Result<Vec<LinearColor>, PaletteError> {
    let mut bins: Vec<Vec<LinearColor>> = vec![pixels];

    // Keep splitting until we have target_count bins
    while bins.len() < target_count {
        // Find the bin with the longest axis range to split
        let (split_idx, _) = bins
            .iter()
            .enumerate()
            .filter(|(_, bin)| bin.len() > 1)
            .map(|(i, bin)| (i, longest_axis_range(bin)))
            .max_by(|(_, a), (_, b)| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((0, (0, 0.0)));

        // If the selected bin has only 1 pixel, we can't split further
        if bins[split_idx].len() <= 1 {
            break;
        }

        let bin = bins.remove(split_idx);
        let (left, right) = split_at_median(bin);

        if left.is_empty() || right.is_empty() {
            // Can't split further; put it back
            if left.is_empty() {
                bins.push(right);
            } else {
                bins.push(left);
            }
            break;
        }

        bins.push(left);
        bins.push(right);
    }

    // Compute mean of each bin
    let palette: Vec<LinearColor> = bins.iter().map(|bin| mean_color(bin)).collect();

    Ok(palette)
}

/// Find the axis with the longest range and return (axis_index, range).
/// axis: 0=R, 1=G, 2=B
fn longest_axis_range(pixels: &[LinearColor]) -> (usize, f32) {
    let (mut min_r, mut min_g, mut min_b) = (f32::MAX, f32::MAX, f32::MAX);
    let (mut max_r, mut max_g, mut max_b) = (f32::MIN, f32::MIN, f32::MIN);

    for p in pixels {
        min_r = min_r.min(p.r);
        max_r = max_r.max(p.r);
        min_g = min_g.min(p.g);
        max_g = max_g.max(p.g);
        min_b = min_b.min(p.b);
        max_b = max_b.max(p.b);
    }

    let range_r = max_r - min_r;
    let range_g = max_g - min_g;
    let range_b = max_b - min_b;

    if range_r >= range_g && range_r >= range_b {
        (0, range_r)
    } else if range_g >= range_b {
        (1, range_g)
    } else {
        (2, range_b)
    }
}

/// Split a bin at the median along the longest axis.
fn split_at_median(mut pixels: Vec<LinearColor>) -> (Vec<LinearColor>, Vec<LinearColor>) {
    let (axis, _) = longest_axis_range(&pixels);

    // Sort along the chosen axis
    pixels.sort_by(|a, b| {
        let va = channel_value(a, axis);
        let vb = channel_value(b, axis);
        va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mid = pixels.len() / 2;
    let right = pixels.split_off(mid);
    (pixels, right)
}

/// Get the value of a specific channel (0=R, 1=G, 2=B).
fn channel_value(color: &LinearColor, axis: usize) -> f32 {
    match axis {
        0 => color.r,
        1 => color.g,
        _ => color.b,
    }
}

/// Compute the mean color of a bin.
fn mean_color(pixels: &[LinearColor]) -> LinearColor {
    let len = pixels.len() as f32;
    let (sum_r, sum_g, sum_b) =
        pixels
            .iter()
            .fold((0.0f32, 0.0f32, 0.0f32), |(r, g, b), p| {
                (r + p.r, g + p.g, b + p.b)
            });
    LinearColor {
        r: sum_r / len,
        g: sum_g / len,
        b: sum_b / len,
    }
}

// =============================================================================
// K-Means
// =============================================================================

/// K-Means palette generation with k-means++ initialization.
///
/// 1. Initialize centroids via k-means++
/// 2. Iterate: assign each pixel to nearest centroid, recompute centroid as mean
/// 3. Stop when max centroid movement < 1e-4 or 50 iterations
fn kmeans(
    pixels: Vec<LinearColor>,
    target_count: usize,
) -> Result<Vec<LinearColor>, PaletteError> {
    let k = target_count;

    // k-means++ initialization
    let mut centroids = kmeans_pp_init(&pixels, k);

    let max_iterations = 50;
    let convergence_threshold: f32 = 1e-4;

    for _ in 0..max_iterations {
        // Assign each pixel to nearest centroid
        let mut assignments: Vec<Vec<LinearColor>> = vec![Vec::new(); k];
        for pixel in &pixels {
            let nearest = find_nearest_centroid(pixel, &centroids);
            assignments[nearest].push(*pixel);
        }

        // Recompute centroids
        let mut max_movement: f32 = 0.0;
        for (i, cluster) in assignments.iter().enumerate() {
            if cluster.is_empty() {
                // Keep the old centroid if no pixels assigned
                continue;
            }
            let new_centroid = mean_color(cluster);
            let movement = color_dist_sq(&centroids[i], &new_centroid).sqrt();
            max_movement = max_movement.max(movement);
            centroids[i] = new_centroid;
        }

        // Check convergence
        if max_movement < convergence_threshold {
            break;
        }
    }

    Ok(centroids)
}

/// K-means++ initialization: pick the first centroid as the first pixel (for determinism),
/// then pick subsequent centroids weighted by distance to nearest existing centroid.
fn kmeans_pp_init(pixels: &[LinearColor], k: usize) -> Vec<LinearColor> {
    let mut centroids: Vec<LinearColor> = Vec::with_capacity(k);

    // First centroid: use first pixel for determinism in tests
    centroids.push(pixels[0]);

    for _ in 1..k {
        // For each pixel, compute distance to nearest existing centroid
        let distances: Vec<f32> = pixels
            .iter()
            .map(|p| {
                centroids
                    .iter()
                    .map(|c| color_dist_sq(p, c))
                    .fold(f32::MAX, f32::min)
            })
            .collect();

        // Compute cumulative sum for weighted selection
        let total: f32 = distances.iter().sum();
        if total <= 0.0 {
            // All pixels are at existing centroids, just duplicate
            centroids.push(pixels[0]);
            continue;
        }

        // Normalize to probabilities and use cumulative sum for deterministic selection
        // Pick the pixel with the maximum distance (deterministic alternative to random weighted)
        let max_idx = distances
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);

        centroids.push(pixels[max_idx]);
    }

    centroids
}

/// Find the index of the nearest centroid to a pixel.
fn find_nearest_centroid(pixel: &LinearColor, centroids: &[LinearColor]) -> usize {
    centroids
        .iter()
        .enumerate()
        .map(|(i, c)| (i, color_dist_sq(pixel, c)))
        .min_by(|(_, da), (_, db)| da.partial_cmp(db).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Squared Euclidean distance between two LinearColors in RGB space.
fn color_dist_sq(a: &LinearColor, b: &LinearColor) -> f32 {
    let dr = a.r - b.r;
    let dg = a.g - b.g;
    let db = a.b - b.b;
    dr * dr + dg * dg + db * db
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_median_cut_two_colors_from_red_blue() {
        // Input: many reds and many blues
        let mut pixels = Vec::new();
        for _ in 0..100 {
            pixels.push(LinearColor {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            });
        }
        for _ in 0..100 {
            pixels.push(LinearColor {
                r: 0.0,
                g: 0.0,
                b: 1.0,
            });
        }

        let result =
            generate_palette(pixels.into_iter(), 2, PaletteGenMethod::MedianCut).unwrap();

        assert_eq!(result.len(), 2);

        // One should be close to red, the other close to blue
        let has_red = result
            .iter()
            .any(|c| c.r > 0.9 && c.g < 0.1 && c.b < 0.1);
        let has_blue = result
            .iter()
            .any(|c| c.r < 0.1 && c.g < 0.1 && c.b > 0.9);
        assert!(has_red, "Expected a red-ish color in palette: {:?}", result);
        assert!(
            has_blue,
            "Expected a blue-ish color in palette: {:?}",
            result
        );
    }

    #[test]
    fn test_kmeans_convergence() {
        // Input: two well-separated clusters
        let mut pixels = Vec::new();
        for i in 0..50 {
            let offset = (i as f32) * 0.001;
            pixels.push(LinearColor {
                r: 0.9 + offset,
                g: 0.0,
                b: 0.0,
            });
        }
        for i in 0..50 {
            let offset = (i as f32) * 0.001;
            pixels.push(LinearColor {
                r: 0.0,
                g: 0.0,
                b: 0.9 + offset,
            });
        }

        let result = generate_palette(pixels.into_iter(), 2, PaletteGenMethod::KMeans).unwrap();

        assert_eq!(result.len(), 2);

        // Centroids should converge near the cluster centers
        let has_red = result.iter().any(|c| c.r > 0.8 && c.b < 0.2);
        let has_blue = result.iter().any(|c| c.b > 0.8 && c.r < 0.2);
        assert!(
            has_red,
            "Expected a red cluster centroid: {:?}",
            result
        );
        assert!(
            has_blue,
            "Expected a blue cluster centroid: {:?}",
            result
        );
    }

    #[test]
    fn test_empty_input_error() {
        let pixels: Vec<LinearColor> = Vec::new();
        let result = generate_palette(pixels.into_iter(), 4, PaletteGenMethod::MedianCut);
        assert!(result.is_err());
        match result {
            Err(PaletteError::GenerationFailed(msg)) => {
                assert!(msg.contains("no pixels"));
            }
            other => panic!("Expected GenerationFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_kmeans_empty_input_error() {
        let pixels: Vec<LinearColor> = Vec::new();
        let result = generate_palette(pixels.into_iter(), 4, PaletteGenMethod::KMeans);
        assert!(result.is_err());
        match result {
            Err(PaletteError::GenerationFailed(msg)) => {
                assert!(msg.contains("no pixels"));
            }
            other => panic!("Expected GenerationFailed, got {:?}", other),
        }
    }

    #[test]
    fn test_fewer_unique_than_target() {
        // Only 2 unique colors but requesting 8
        let pixels = vec![
            LinearColor {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            },
            LinearColor {
                r: 0.0,
                g: 1.0,
                b: 0.0,
            },
            LinearColor {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            },
            LinearColor {
                r: 0.0,
                g: 1.0,
                b: 0.0,
            },
        ];

        let result =
            generate_palette(pixels.into_iter(), 8, PaletteGenMethod::MedianCut).unwrap();

        // Should return only 2 unique colors
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_kmeans_fewer_unique_than_target() {
        // Only 3 unique colors but requesting 10
        let pixels = vec![
            LinearColor { r: 1.0, g: 0.0, b: 0.0 },
            LinearColor { r: 0.0, g: 1.0, b: 0.0 },
            LinearColor { r: 0.0, g: 0.0, b: 1.0 },
            LinearColor { r: 1.0, g: 0.0, b: 0.0 },
            LinearColor { r: 0.0, g: 1.0, b: 0.0 },
        ];

        let result =
            generate_palette(pixels.into_iter(), 10, PaletteGenMethod::KMeans).unwrap();

        assert_eq!(result.len(), 3);
    }
}
