//! Cell-space dithering: ordered thresholds and vector error diffusion.

use crate::descriptor::{ShapeDesc, ToneDesc};

/// Ordered / diffusion dither applied in cell space before matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CellDither {
    #[default]
    None,
    Bayer2,
    Bayer4,
    Bayer8,
    /// Floyd–Steinberg on the descriptor residual (CPU serial).
    FloydSteinberg { serpentine: bool },
}

const BAYER_2: [[u16; 2]; 2] = [[0, 2], [3, 1]];
const BAYER_4: [[u16; 4]; 4] = [
    [0, 8, 2, 10],
    [12, 4, 14, 6],
    [3, 11, 1, 9],
    [15, 7, 13, 5],
];
// Standard Bayer-8 ranks 0..=63.
const BAYER_8: [[u16; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

fn ordered_bias_u16(col: u32, row: u32, dither: CellDither) -> i32 {
    // Map Bayer rank → bias in coverage units ≈ ±½ step of the matrix.
    let (rank, n) = match dither {
        CellDither::Bayer2 => (
            BAYER_2[(row % 2) as usize][(col % 2) as usize] as i32,
            4i32,
        ),
        CellDither::Bayer4 => (
            BAYER_4[(row % 4) as usize][(col % 4) as usize] as i32,
            16i32,
        ),
        CellDither::Bayer8 => (
            BAYER_8[(row % 8) as usize][(col % 8) as usize] as i32,
            64i32,
        ),
        CellDither::None | CellDither::FloydSteinberg { .. } => return 0,
    };
    // Bias in 0..=65535 space: (rank / n − 0.5) × (65535 / n) roughly one level.
    let centered = rank * 2 + 1 - n; // odd ranks centred around 0
    (centered * 65535) / (n * n)
}

/// Apply ordered threshold bias to a tone descriptor.
pub fn dither_tone(tone: ToneDesc, col: u32, row: u32, dither: CellDither) -> ToneDesc {
    let bias = ordered_bias_u16(col, row, dither);
    let v = (tone.coverage as i32 + bias).clamp(0, 65535) as u16;
    ToneDesc { coverage: v }
}

/// Apply ordered threshold bias to each shape sample.
pub fn dither_shape(shape: ShapeDesc, col: u32, row: u32, dither: CellDither) -> ShapeDesc {
    let bias = ordered_bias_u16(col, row, dither);
    let mut samples = shape.samples;
    for s in &mut samples {
        *s = (*s as i32 + bias).clamp(0, 65535) as u16;
    }
    ShapeDesc { samples }
}

/// Mutable error buffer for vector FS diffusion over a `cols × rows` grid.
pub struct ToneErrorBuf {
    cols: u32,
    /// Per-cell residual (i32 coverage units), size cols*(rows+1) for next-row spill.
    err: Vec<i32>,
}

impl ToneErrorBuf {
    pub fn new(cols: u32, rows: u32) -> Self {
        Self {
            cols,
            err: vec![0; (cols * (rows + 1)) as usize],
        }
    }

    fn idx(&self, col: u32, row: u32) -> usize {
        (row * self.cols + col) as usize
    }

    pub fn take_tone(&mut self, tone: ToneDesc, col: u32, row: u32) -> ToneDesc {
        let e = self.err[self.idx(col, row)];
        let v = (tone.coverage as i32 + e).clamp(0, 65535) as u16;
        ToneDesc { coverage: v }
    }

    /// Diffuse residual after matching (`chosen` coverage vs pre-match tone).
    pub fn diffuse(
        &mut self,
        pre: ToneDesc,
        chosen: ToneDesc,
        col: u32,
        row: u32,
        cols: u32,
        rows: u32,
        serpentine: bool,
        row_ltr: bool,
    ) {
        let residual = pre.coverage as i32 - chosen.coverage as i32;
        if residual == 0 {
            return;
        }
        let forward = if serpentine && !row_ltr { -1i32 } else { 1 };
        // FS: 7/16 right, 3/16 below-left, 5/16 below, 1/16 below-right (mirrored when RTL).
        let push = |buf: &mut ToneErrorBuf, c: i32, r: u32, w: i32| {
            if c < 0 || c as u32 >= cols || r >= rows {
                return;
            }
            let idx = buf.idx(c as u32, r);
            let add = residual * w / 16;
            buf.err[idx] += add;
        };
        push(self, col as i32 + forward, row, 7);
        push(self, col as i32 - forward, row + 1, 3);
        push(self, col as i32, row + 1, 5);
        push(self, col as i32 + forward, row + 1, 1);
    }
}

/// Shape (6-D) error buffer for vector FS.
pub struct ShapeErrorBuf {
    cols: u32,
    err: Vec<[i32; 6]>,
}

impl ShapeErrorBuf {
    pub fn new(cols: u32, rows: u32) -> Self {
        Self {
            cols,
            err: vec![[0; 6]; (cols * (rows + 1)) as usize],
        }
    }

    fn idx(&self, col: u32, row: u32) -> usize {
        (row * self.cols + col) as usize
    }

    pub fn take_shape(&mut self, shape: ShapeDesc, col: u32, row: u32) -> ShapeDesc {
        let e = self.err[self.idx(col, row)];
        let mut samples = [0u16; 6];
        for i in 0..6 {
            samples[i] = (shape.samples[i] as i32 + e[i]).clamp(0, 65535) as u16;
        }
        ShapeDesc { samples }
    }

    pub fn diffuse(
        &mut self,
        pre: ShapeDesc,
        chosen: ShapeDesc,
        col: u32,
        row: u32,
        cols: u32,
        rows: u32,
        serpentine: bool,
        row_ltr: bool,
    ) {
        let mut residual = [0i32; 6];
        let mut any = false;
        for i in 0..6 {
            residual[i] = pre.samples[i] as i32 - chosen.samples[i] as i32;
            any |= residual[i] != 0;
        }
        if !any {
            return;
        }
        let forward = if serpentine && !row_ltr { -1i32 } else { 1 };
        let push = |buf: &mut ShapeErrorBuf, c: i32, r: u32, w: i32| {
            if c < 0 || c as u32 >= cols || r >= rows {
                return;
            }
            let idx = buf.idx(c as u32, r);
            for i in 0..6 {
                buf.err[idx][i] += residual[i] * w / 16;
            }
        };
        push(self, col as i32 + forward, row, 7);
        push(self, col as i32 - forward, row + 1, 3);
        push(self, col as i32, row + 1, 5);
        push(self, col as i32 + forward, row + 1, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_bias_centered() {
        // Rank 0 of Bayer2 → negative bias; rank 3 → positive.
        let lo = dither_tone(ToneDesc { coverage: 32768 }, 0, 0, CellDither::Bayer2);
        let hi = dither_tone(ToneDesc { coverage: 32768 }, 0, 1, CellDither::Bayer2);
        assert!(lo.coverage < 32768);
        assert!(hi.coverage > 32768);
    }

    #[test]
    fn fs_energy_moves_to_neighbours() {
        let mut buf = ToneErrorBuf::new(3, 2);
        let pre = ToneDesc { coverage: 40000 };
        let chosen = ToneDesc { coverage: 30000 };
        buf.diffuse(pre, chosen, 1, 0, 3, 2, false, true);
        // Residual 10000 → 7/16 right, 3/16 below-left, 5/16 below, 1/16 below-right.
        assert_eq!(buf.err[buf.idx(2, 0)], 10000 * 7 / 16);
        assert_eq!(buf.err[buf.idx(0, 1)], 10000 * 3 / 16);
        assert_eq!(buf.err[buf.idx(1, 1)], 10000 * 5 / 16);
        assert_eq!(buf.err[buf.idx(2, 1)], 10000 * 1 / 16);
    }
}
