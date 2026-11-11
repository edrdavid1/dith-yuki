//! `.cube` LUT — pointwise Post color filter.
//!
//! Empty `path` or `amount == 0` is a bitwise no-op. Otherwise samples the
//! Adobe 3D cube (cached) and lerps toward the LUT result by `amount`.

use std::path::Path;

use engine_color::{global_cube_lut_cache, CubeLut3D};
use engine_tiles::{PixelTile, HALO, TILE_SIZE};
use log::warn;

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;

/// Apply cube LUT into `dst`. `src` and `dst` must not alias.
pub fn apply_cube_lut_into(src: &PixelTile, path: &str, amount: f32, dst: &mut PixelTile) {
    debug_assert_ne!(src.data.as_ptr(), dst.data.as_ptr());
    let amount = amount.clamp(0.0, 1.0);
    if path.trim().is_empty() || amount == 0.0 {
        dst.copy_from(src);
        return;
    }
    let lut = match global_cube_lut_cache().get_or_load(Path::new(path)) {
        Ok(lut) => lut,
        Err(e) => {
            warn!("cube LUT load failed ({path}): {e}; passing through");
            dst.copy_from(src);
            return;
        }
    };
    apply_cube_lut_loaded(src, &lut, amount, dst);
}

pub fn apply_cube_lut_loaded(src: &PixelTile, lut: &CubeLut3D, amount: f32, dst: &mut PixelTile) {
    let amount = amount.clamp(0.0, 1.0);
    if amount == 0.0 {
        dst.copy_from(src);
        return;
    }
    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            let r = src.at(x, y, 0);
            let g = src.at(x, y, 1);
            let b = src.at(x, y, 2);
            let a = src.at(x, y, 3);
            let (or, og, ob, oa) = apply_cube_lut_point_rgba(r, g, b, a, lut, amount);
            dst.set(x, y, 0, or);
            dst.set(x, y, 1, og);
            dst.set(x, y, 2, ob);
            dst.set(x, y, 3, oa);
        }
    }
}

#[inline]
pub fn apply_cube_lut_point_rgba(
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    lut: &CubeLut3D,
    amount: f32,
) -> (f32, f32, f32, f32) {
    let out = lut.lookup(r, g, b);
    if amount >= 1.0 {
        return (out[0], out[1], out[2], a);
    }
    (
        r + (out[0] - r) * amount,
        g + (out[1] - g) * amount,
        b + (out[2] - b) * amount,
        a,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_color::CubeLut3D;

    const IDENTITY_2: &str = r#"
LUT_3D_SIZE 2
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
    fn empty_path_is_noop() {
        let mut src = PixelTile::new();
        src.set(HALO, HALO, 0, 0.3);
        src.set(HALO, HALO, 1, 0.4);
        src.set(HALO, HALO, 2, 0.5);
        src.set(HALO, HALO, 3, 0.9);
        let mut dst = PixelTile::new();
        apply_cube_lut_into(&src, "", 1.0, &mut dst);
        assert_eq!(dst.data, src.data);
    }

    #[test]
    fn identity_cube_preserves_mid_gray() {
        let lut = CubeLut3D::parse(IDENTITY_2.as_bytes()).unwrap();
        let mut src = PixelTile::new();
        src.set(HALO, HALO, 0, 0.5);
        src.set(HALO, HALO, 1, 0.5);
        src.set(HALO, HALO, 2, 0.5);
        src.set(HALO, HALO, 3, 1.0);
        let mut dst = PixelTile::new();
        apply_cube_lut_loaded(&src, &lut, 1.0, &mut dst);
        assert!((dst.at(HALO, HALO, 0) - 0.5).abs() < 1e-4);
        assert!((dst.at(HALO, HALO, 1) - 0.5).abs() < 1e-4);
        assert!((dst.at(HALO, HALO, 2) - 0.5).abs() < 1e-4);
        assert_eq!(dst.at(HALO, HALO, 3), 1.0);
    }

    #[test]
    fn amount_zero_with_lut_is_noop() {
        let lut = CubeLut3D::parse(IDENTITY_2.as_bytes()).unwrap();
        let mut src = PixelTile::new();
        src.set(HALO, HALO, 0, 0.2);
        src.set(HALO, HALO, 1, 0.3);
        src.set(HALO, HALO, 2, 0.4);
        src.set(HALO, HALO, 3, 0.7);
        let mut dst = PixelTile::new();
        apply_cube_lut_loaded(&src, &lut, 0.0, &mut dst);
        assert_eq!(dst.data, src.data);
    }
}
