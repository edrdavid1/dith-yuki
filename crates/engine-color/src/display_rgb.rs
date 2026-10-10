//! Typed display-referred vs optical-linear RGB so soft-proof cannot mix them.
//!
//! Composite / decode buffers store **display-referred** values (`u8/255`,
//! sRGB-like). Optical linear light is only for APIs that explicitly convert
//! (e.g. true-linear test helpers).

/// One pixel of display-referred RGB in [0, 1] (Composite / `u8/255` convention).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayRgbF32(pub [f32; 3]);

impl DisplayRgbF32 {
    #[inline]
    pub fn new(r: f32, g: f32, b: f32) -> Self {
        Self([r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)])
    }

    #[inline]
    pub fn channels(self) -> [f32; 3] {
        self.0
    }

    #[inline]
    pub fn r(self) -> f32 {
        self.0[0]
    }
    #[inline]
    pub fn g(self) -> f32 {
        self.0[1]
    }
    #[inline]
    pub fn b(self) -> f32 {
        self.0[2]
    }

    /// Pack interleaved display RGB into typed pixels (length must be multiple of 3).
    pub fn pack_from_interleaved(rgb: &[f32]) -> Result<Vec<Self>, ()> {
        if rgb.len() % 3 != 0 {
            return Err(());
        }
        Ok(rgb
            .chunks_exact(3)
            .map(|c| Self::new(c[0], c[1], c[2]))
            .collect())
    }

    /// Unpack typed pixels to interleaved f32 RGB.
    pub fn unpack_to_interleaved(pixels: &[Self]) -> Vec<f32> {
        let mut out = Vec::with_capacity(pixels.len() * 3);
        for p in pixels {
            out.extend_from_slice(&p.0);
        }
        out
    }
}

/// One pixel of optical linear RGB in [0, 1] (scene-referred).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearRgbF32(pub [f32; 3]);

impl LinearRgbF32 {
    #[inline]
    pub fn new(r: f32, g: f32, b: f32) -> Self {
        Self([r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)])
    }

    #[inline]
    pub fn channels(self) -> [f32; 3] {
        self.0
    }
}
