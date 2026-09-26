//! Integer descriptors for glyphs and image cells (CPU reference).
//!
//! Coverage and sample sums use fixed-point integers so a future GPU path can
//! match bit-for-bit (§6.2 of the ASCII system spec).

/// Mean coverage of a `w × h` grayscale buffer, scaled to `0..=65535`.
#[inline]
pub fn mean_coverage(buf: &[u8], w: u32, h: u32) -> u16 {
    debug_assert_eq!(buf.len(), (w * h) as usize);
    if w == 0 || h == 0 {
        return 0;
    }
    let sum: u64 = buf.iter().map(|&v| v as u64).sum();
    // sum / (w*h) in 0..=255, then × 257 → 0..=65535.
    ((sum * 257) / (w as u64 * h as u64)) as u16
}

/// 1-D tone descriptor (mean coverage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToneDesc {
    pub coverage: u16,
}

impl ToneDesc {
    pub fn from_coverage(buf: &[u8], w: u32, h: u32) -> Self {
        Self {
            coverage: mean_coverage(buf, w, h),
        }
    }

    /// Absolute difference on the coverage axis.
    pub fn distance(self, other: Self) -> u32 {
        (self.coverage as i32 - other.coverage as i32).unsigned_abs()
    }
}

/// Six staggered sampling circles (Alex Harri shape vector).
///
/// Layout inside the cell (centres as fractions of width/height):
/// ```text
///      (0.25, 0.20)  (0.75, 0.20)
///   (0.15, 0.50)        (0.85, 0.50)
///      (0.25, 0.80)  (0.75, 0.80)
/// ```
/// Radius = 18% of `min(w, h)`. Each component is mean coverage inside the
/// circle, `0..=65535`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapeDesc {
    pub samples: [u16; 6],
}

const SHAPE_CENTRES: [(f32, f32); 6] = [
    (0.25, 0.20),
    (0.75, 0.20),
    (0.15, 0.50),
    (0.85, 0.50),
    (0.25, 0.80),
    (0.75, 0.80),
];

impl ShapeDesc {
    pub fn from_coverage(buf: &[u8], w: u32, h: u32) -> Self {
        let mut samples = [0u16; 6];
        if w == 0 || h == 0 {
            return Self { samples };
        }
        let r = 0.18 * (w.min(h) as f32);
        let r2 = r * r;
        for (i, &(fx, fy)) in SHAPE_CENTRES.iter().enumerate() {
            samples[i] = sample_circle(buf, w, h, fx * w as f32, fy * h as f32, r2);
        }
        Self { samples }
    }

    /// Squared L2 distance in fixed-point sample space.
    pub fn distance2(self, other: Self) -> u64 {
        let mut acc = 0u64;
        for i in 0..6 {
            let d = self.samples[i] as i64 - other.samples[i] as i64;
            acc += (d * d) as u64;
        }
        acc
    }
}

/// Mean coverage inside a circle, supersampled 2×2 for partial pixels.
fn sample_circle(buf: &[u8], w: u32, h: u32, cx: f32, cy: f32, r2: f32) -> u16 {
    const SS: u32 = 2;
    let rad = r2.sqrt();
    let x0 = (cx - rad).floor().max(0.0) as i32;
    let y0 = (cy - rad).floor().max(0.0) as i32;
    let x1 = ((cx + rad).ceil() as i32).min(w as i32);
    let y1 = ((cy + rad).ceil() as i32).min(h as i32);
    if x0 >= x1 || y0 >= y1 {
        return 0;
    }
    let mut sum = 0u64;
    let mut weight = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            let mut hits = 0u32;
            for sy in 0..SS {
                for sx in 0..SS {
                    let px = x as f32 + (sx as f32 + 0.5) / SS as f32 - cx;
                    let py = y as f32 + (sy as f32 + 0.5) / SS as f32 - cy;
                    if px * px + py * py <= r2 {
                        hits += 1;
                    }
                }
            }
            if hits == 0 {
                continue;
            }
            let v = buf[(y as u32 * w + x as u32) as usize] as u64;
            sum += v * hits as u64;
            weight += hits as u64;
        }
    }
    if weight == 0 {
        return 0;
    }
    ((sum * 257) / weight) as u16
}

/// Shape descriptor with optional Harri-style contrast stretch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapeContrastDesc {
    pub shape: ShapeDesc,
}

impl ShapeContrastDesc {
    /// Raise each sample toward 0 or 65535 by exponent `gamma` (1 = identity).
    ///
    /// `gamma` is stored as fixed 8.8 (`256` = 1.0). Applied in float only for
    /// the power; result is re-quantized to u16.
    pub fn from_shape(shape: ShapeDesc, gamma_8_8: u16) -> Self {
        if gamma_8_8 == 256 {
            return Self { shape };
        }
        let g = gamma_8_8 as f32 / 256.0;
        let samples = shape.samples.map(|s| {
            let t = s as f32 / 65535.0;
            let e = t.powf(g).clamp(0.0, 1.0);
            (e * 65535.0).round() as u16
        });
        Self {
            shape: ShapeDesc { samples },
        }
    }

    pub fn distance2(self, other: Self) -> u64 {
        self.shape.distance2(other.shape)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, v: u8) -> Vec<u8> {
        vec![v; (w * h) as usize]
    }

    #[test]
    fn mean_coverage_extrema() {
        assert_eq!(mean_coverage(&solid(8, 8, 0), 8, 8), 0);
        assert_eq!(mean_coverage(&solid(8, 8, 255), 8, 8), 65535);
        let half = mean_coverage(&solid(4, 4, 128), 4, 4);
        assert!((half as i32 - 128 * 257).abs() <= 1);
    }

    #[test]
    fn tone_distance_symmetric() {
        let a = ToneDesc { coverage: 1000 };
        let b = ToneDesc { coverage: 5000 };
        assert_eq!(a.distance(b), b.distance(a));
        assert_eq!(a.distance(a), 0);
    }

    #[test]
    fn shape_empty_vs_full() {
        let empty = ShapeDesc::from_coverage(&solid(16, 16, 0), 16, 16);
        let full = ShapeDesc::from_coverage(&solid(16, 16, 255), 16, 16);
        assert!(empty.samples.iter().all(|&s| s == 0));
        assert!(full.samples.iter().all(|&s| s == 65535));
        assert!(empty.distance2(full) > 0);
    }

    #[test]
    fn shape_detects_left_vs_right() {
        let mut left = solid(20, 20, 0);
        for y in 0..20 {
            for x in 0..10 {
                left[y * 20 + x] = 255;
            }
        }
        let mut right = solid(20, 20, 0);
        for y in 0..20 {
            for x in 10..20 {
                right[y * 20 + x] = 255;
            }
        }
        let l = ShapeDesc::from_coverage(&left, 20, 20);
        let r = ShapeDesc::from_coverage(&right, 20, 20);
        // Left-column samples (0, 2, 4) darker on right-filled image.
        assert!(l.samples[0] > r.samples[0]);
        assert!(l.samples[2] > r.samples[2]);
        assert!(r.samples[1] > l.samples[1]);
        assert!(r.samples[3] > l.samples[3]);
    }

    #[test]
    fn contrast_gamma_stretches() {
        let mid = ShapeDesc {
            samples: [32768; 6],
        };
        let stretched = ShapeContrastDesc::from_shape(mid, 512); // gamma 2
        assert!(stretched.shape.samples[0] < mid.samples[0]);
        let identity = ShapeContrastDesc::from_shape(mid, 256);
        assert_eq!(identity.shape, mid);
    }
}
