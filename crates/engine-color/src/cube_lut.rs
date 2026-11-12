//! Adobe `.cube` 3D LUT parse, tetrahedral sample, and path cache.
//!
//! Spec: Adobe Cube LUT Specification 1.0 (3D tables only). Table order is
//! red-fastest (`index = r + N·g + N²·b`). Max size capped at 65 for RAM.

use dashmap::DashMap;
use engine_io::sandbox::{self, SandboxError};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;
use thiserror::Error;

/// Hard cap on `LUT_3D_SIZE` (65³ × 3 × f32 ≈ 3.3 MiB).
pub const MAX_CUBE_SIZE: usize = 65;

const MAX_CACHE_ENTRIES: usize = 32;

#[derive(Debug, Error)]
pub enum CubeLutError {
    #[error("cube LUT: {0}")]
    Parse(String),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("sandbox error: {0}")]
    Sandbox(#[from] SandboxError),
}

/// Parsed 3D cube LUT ready for sampling.
#[derive(Debug, Clone)]
pub struct CubeLut3D {
    pub size: usize,
    pub title: Option<String>,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    /// Interleaved RGB, Adobe order (R fastest). Length `size³ * 3`.
    data: Vec<f32>,
}

impl CubeLut3D {
    /// Parse an Adobe `.cube` text body (UTF-8 / Latin-1 bytes).
    pub fn parse(bytes: &[u8]) -> Result<Self, CubeLutError> {
        let owned;
        let text = match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                owned = String::from_utf8_lossy(bytes).into_owned();
                owned.as_str()
            }
        };
        parse_cube_text(text)
    }

    #[inline]
    fn sample(&self, r: usize, g: usize, b: usize) -> [f32; 3] {
        let n = self.size;
        // Adobe: R fastest → index = r + N*g + N²*b
        let i = (r + n * (g + n * b)) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    /// Interleaved RGB samples (Adobe order, R fastest). Length `size³ * 3`.
    pub fn rgb_data(&self) -> &[f32] {
        &self.data
    }

    /// Map input RGB through the LUT (domain remap + tetrahedral interpolation).
    pub fn lookup(&self, r: f32, g: f32, b: f32) -> [f32; 3] {
        let n = self.size;
        let max = (n - 1) as f32;
        let map = |v: f32, lo: f32, hi: f32| {
            if (hi - lo).abs() < 1e-12 {
                0.0
            } else {
                ((v - lo) / (hi - lo)).clamp(0.0, 1.0) * max
            }
        };
        let rf = map(r, self.domain_min[0], self.domain_max[0]);
        let gf = map(g, self.domain_min[1], self.domain_max[1]);
        let bf = map(b, self.domain_min[2], self.domain_max[2]);

        let r0 = rf.floor() as usize;
        let g0 = gf.floor() as usize;
        let b0 = bf.floor() as usize;
        let r1 = (r0 + 1).min(n - 1);
        let g1 = (g0 + 1).min(n - 1);
        let b1 = (b0 + 1).min(n - 1);
        let tr = rf - r0 as f32;
        let tg = gf - g0 as f32;
        let tb = bf - b0 as f32;

        let c000 = self.sample(r0, g0, b0);
        let c100 = self.sample(r1, g0, b0);
        let c010 = self.sample(r0, g1, b0);
        let c001 = self.sample(r0, g0, b1);
        let c110 = self.sample(r1, g1, b0);
        let c101 = self.sample(r1, g0, b1);
        let c011 = self.sample(r0, g1, b1);
        let c111 = self.sample(r1, g1, b1);

        // Same tetrahedral cases as SoftProofLut3D (CMS standard).
        if tr > tg {
            if tg > tb {
                lerp3(c000, c100, c110, c111, tr, tg, tb)
            } else if tr > tb {
                lerp3(c000, c100, c101, c111, tr, tb, tg)
            } else {
                lerp3(c000, c001, c101, c111, tb, tr, tg)
            }
        } else if tb > tg {
            lerp3(c000, c001, c011, c111, tb, tg, tr)
        } else if tb > tr {
            lerp3(c000, c010, c011, c111, tg, tb, tr)
        } else {
            lerp3(c000, c010, c110, c111, tg, tr, tb)
        }
    }
}

#[inline]
fn lerp3(
    c0: [f32; 3],
    c1: [f32; 3],
    c2: [f32; 3],
    c3: [f32; 3],
    t1: f32,
    t2: f32,
    t3: f32,
) -> [f32; 3] {
    [
        c0[0] + t1 * (c1[0] - c0[0]) + t2 * (c2[0] - c1[0]) + t3 * (c3[0] - c2[0]),
        c0[1] + t1 * (c1[1] - c0[1]) + t2 * (c2[1] - c1[1]) + t3 * (c3[1] - c2[1]),
        c0[2] + t1 * (c1[2] - c0[2]) + t2 * (c2[2] - c1[2]) + t3 * (c3[2] - c2[2]),
    ]
}

fn parse_cube_text(text: &str) -> Result<CubeLut3D, CubeLutError> {
    let mut title: Option<String> = None;
    let mut size: Option<usize> = None;
    let mut domain_min = [0.0_f32, 0.0, 0.0];
    let mut domain_max = [1.0_f32, 1.0, 1.0];
    let mut data: Vec<f32> = Vec::new();
    let mut in_table = false;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let upper = line.to_ascii_uppercase();
        if !in_table {
            if upper.starts_with("TITLE") {
                title = Some(parse_title(line)?);
                continue;
            }
            if upper.starts_with("LUT_3D_SIZE") {
                let n = parse_usize_after_keyword(line, "LUT_3D_SIZE")?;
                if !(2..=MAX_CUBE_SIZE).contains(&n) {
                    return Err(CubeLutError::Parse(format!(
                        "LUT_3D_SIZE {n} out of range [2, {MAX_CUBE_SIZE}]"
                    )));
                }
                size = Some(n);
                continue;
            }
            if upper.starts_with("LUT_1D_SIZE") {
                return Err(CubeLutError::Parse(
                    "1D cube LUTs are not supported (need LUT_3D_SIZE)".into(),
                ));
            }
            if upper.starts_with("DOMAIN_MIN") {
                domain_min = parse_rgb_after_keyword(line, "DOMAIN_MIN")?;
                continue;
            }
            if upper.starts_with("DOMAIN_MAX") {
                domain_max = parse_rgb_after_keyword(line, "DOMAIN_MAX")?;
                continue;
            }
            if upper.starts_with("LUT_3D_INPUT_RANGE") {
                let (lo, hi) = parse_range_after_keyword(line, "LUT_3D_INPUT_RANGE")?;
                domain_min = [lo, lo, lo];
                domain_max = [hi, hi, hi];
                continue;
            }
            // First data line → table mode.
            if let Ok(rgb) = parse_rgb_triplet(line) {
                let Some(n) = size else {
                    return Err(CubeLutError::Parse(
                        "table data before LUT_3D_SIZE".into(),
                    ));
                };
                data.reserve(n * n * n * 3);
                data.extend_from_slice(&rgb);
                in_table = true;
                continue;
            }
            return Err(CubeLutError::Parse(format!(
                "unknown header keyword: {line}"
            )));
        }

        let rgb = parse_rgb_triplet(line)?;
        data.extend_from_slice(&rgb);
    }

    let Some(n) = size else {
        return Err(CubeLutError::Parse("missing LUT_3D_SIZE".into()));
    };
    for c in 0..3 {
        if !(domain_min[c] < domain_max[c]) {
            return Err(CubeLutError::Parse(
                "DOMAIN_MIN must be strictly less than DOMAIN_MAX".into(),
            ));
        }
    }
    let expected = n * n * n * 3;
    if data.len() != expected {
        return Err(CubeLutError::Parse(format!(
            "expected {expected} floats (N³ RGB), got {}",
            data.len()
        )));
    }
    Ok(CubeLut3D {
        size: n,
        title,
        domain_min,
        domain_max,
        data,
    })
}

fn parse_title(line: &str) -> Result<String, CubeLutError> {
    let rest = line.trim_start()["TITLE".len()..].trim_start();
    if let Some(inner) = rest.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return Ok(inner.to_string());
    }
    Ok(rest.to_string())
}

fn parse_usize_after_keyword(line: &str, key: &str) -> Result<usize, CubeLutError> {
    let rest = strip_keyword(line, key)?;
    rest.parse::<usize>()
        .map_err(|_| CubeLutError::Parse(format!("bad integer after {key}")))
}

fn parse_rgb_after_keyword(line: &str, key: &str) -> Result<[f32; 3], CubeLutError> {
    let rest = strip_keyword(line, key)?;
    parse_rgb_triplet(rest)
}

fn parse_range_after_keyword(line: &str, key: &str) -> Result<(f32, f32), CubeLutError> {
    let rest = strip_keyword(line, key)?;
    let mut parts = rest.split_whitespace();
    let lo = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| CubeLutError::Parse(format!("bad range after {key}")))?;
    let hi = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| CubeLutError::Parse(format!("bad range after {key}")))?;
    Ok((lo, hi))
}

fn strip_keyword<'a>(line: &'a str, key: &str) -> Result<&'a str, CubeLutError> {
    let trimmed = line.trim_start();
    if trimmed.len() < key.len() || !trimmed[..key.len()].eq_ignore_ascii_case(key) {
        return Err(CubeLutError::Parse(format!("expected {key}")));
    }
    Ok(trimmed[key.len()..].trim_start())
}

fn parse_rgb_triplet(line: &str) -> Result<[f32; 3], CubeLutError> {
    let mut parts = line.split_whitespace();
    let r = parts
        .next()
        .and_then(|s| s.parse::<f32>().ok())
        .ok_or_else(|| CubeLutError::Parse(format!("bad RGB line: {line}")))?;
    let g = parts
        .next()
        .and_then(|s| s.parse::<f32>().ok())
        .ok_or_else(|| CubeLutError::Parse(format!("bad RGB line: {line}")))?;
    let b = parts
        .next()
        .and_then(|s| s.parse::<f32>().ok())
        .ok_or_else(|| CubeLutError::Parse(format!("bad RGB line: {line}")))?;
    if !r.is_finite() || !g.is_finite() || !b.is_finite() {
        return Err(CubeLutError::Parse("non-finite RGB value".into()));
    }
    Ok([r, g, b])
}

type CubeCacheKey = (PathBuf, SystemTime);

/// Process-wide cache for loaded `.cube` files (path + mtime).
pub struct CubeLutCache {
    entries: DashMap<CubeCacheKey, Arc<CubeLut3D>>,
    lru_order: Mutex<VecDeque<CubeCacheKey>>,
}

impl CubeLutCache {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
            lru_order: Mutex::new(VecDeque::new()),
        }
    }

    pub fn get_or_load(&self, path: &Path) -> Result<Arc<CubeLut3D>, CubeLutError> {
        let path_str = path.to_str().unwrap_or("");
        let canonical = sandbox::resolve_user_path(path_str, &["cube"])?;
        let meta = fs::metadata(&canonical).map_err(|e| CubeLutError::Io(e.to_string()))?;
        let mtime = meta
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let key = (canonical.clone(), mtime);

        if let Some(entry) = self.entries.get(&key) {
            if let Ok(mut lru) = self.lru_order.lock() {
                if let Some(pos) = lru.iter().position(|k| k == &key) {
                    lru.remove(pos);
                }
                lru.push_back(key.clone());
            }
            return Ok(Arc::clone(&entry));
        }

        let bytes = fs::read(&canonical).map_err(|e| CubeLutError::Io(e.to_string()))?;
        let lut = Arc::new(CubeLut3D::parse(&bytes)?);

        {
            let mut lru = self.lru_order.lock().unwrap_or_else(|e| e.into_inner());
            while lru.len() >= MAX_CACHE_ENTRIES {
                if let Some(oldest) = lru.pop_front() {
                    self.entries.remove(&oldest);
                }
            }
            lru.push_back(key.clone());
        }
        self.entries.insert(key, Arc::clone(&lut));
        Ok(lut)
    }
}

impl Default for CubeLutCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared cache used by the CubeLut filter apply path.
pub fn global_cube_lut_cache() -> &'static CubeLutCache {
    static CACHE: OnceLock<CubeLutCache> = OnceLock::new();
    CACHE.get_or_init(CubeLutCache::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDENTITY_2: &str = r#"
TITLE "Identity2"
LUT_3D_SIZE 2
DOMAIN_MIN 0.0 0.0 0.0
DOMAIN_MAX 1.0 1.0 1.0
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
"#;

    #[test]
    fn parse_identity_and_sample_corners() {
        let lut = CubeLut3D::parse(IDENTITY_2.as_bytes()).unwrap();
        assert_eq!(lut.size, 2);
        assert_eq!(lut.title.as_deref(), Some("Identity2"));
        let black = lut.lookup(0.0, 0.0, 0.0);
        assert!((black[0] - 0.0).abs() < 1e-5);
        let white = lut.lookup(1.0, 1.0, 1.0);
        assert!((white[0] - 1.0).abs() < 1e-5);
        let mid = lut.lookup(0.5, 0.5, 0.5);
        assert!((mid[0] - 0.5).abs() < 1e-4);
        assert!((mid[1] - 0.5).abs() < 1e-4);
        assert!((mid[2] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn rejects_1d() {
        let err = CubeLut3D::parse(b"LUT_1D_SIZE 2\n0 0 0\n1 1 1\n").unwrap_err();
        assert!(err.to_string().contains("1D"));
    }

    #[test]
    fn rejects_wrong_count() {
        let err = CubeLut3D::parse(
            b"LUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("expected"));
    }
}
