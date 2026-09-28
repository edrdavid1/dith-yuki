//! Placement of an already-fitted bitmap inside a window.
//!
//! The bitmap's pixel size comes from `dither-thumb` (R1). This only centers
//! that rectangle in the preview pane. It does not pad the thumbnail bitmap.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FitRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Largest rectangle with `src` aspect that fits in `dst`, centered.
pub fn fit_centered(src_w: u32, src_h: u32, dst_w: i32, dst_h: i32) -> FitRect {
    if src_w == 0 || src_h == 0 || dst_w <= 0 || dst_h <= 0 {
        return FitRect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        };
    }
    let src_w = src_w as i64;
    let src_h = src_h as i64;
    let dst_w = dst_w as i64;
    let dst_h = dst_h as i64;
    let (w, h) = if src_w * dst_h >= dst_w * src_h {
        let w = dst_w;
        let h = ((src_h * dst_w) / src_w).max(1);
        (w, h)
    } else {
        let h = dst_h;
        let w = ((src_w * dst_h) / src_h).max(1);
        (w, h)
    };
    FitRect {
        x: ((dst_w - w) / 2) as i32,
        y: ((dst_h - h) / 2) as i32,
        w: w as i32,
        h: h as i32,
    }
}

/// Opaque (alpha == 255) bounding box. `None` when every pixel is translucent.
pub fn opaque_bounds(width: u32, height: u32, rgba: &[u8]) -> Option<(u32, u32, u32, u32)> {
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any = false;
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            if rgba.get(i + 3) == Some(&255) {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if any {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

/// Composite a premultiplied RGBA bitmap into a straight-alpha canvas, centered.
/// Pixels outside the fitted rect stay `bg` (straight RGBA).
pub fn composite_centered(
    src_w: u32,
    src_h: u32,
    src_premul: &[u8],
    dst_w: u32,
    dst_h: u32,
    bg: [u8; 4],
) -> Vec<u8> {
    let mut out = Vec::with_capacity((dst_w as usize) * (dst_h as usize) * 4);
    for _ in 0..(dst_w as usize) * (dst_h as usize) {
        out.extend_from_slice(&bg);
    }
    let fit = fit_centered(src_w, src_h, dst_w as i32, dst_h as i32);
    if fit.w <= 0 || fit.h <= 0 || src_w == 0 || src_h == 0 {
        return out;
    }
    for y in 0..fit.h {
        for x in 0..fit.w {
            let sx = ((x as i64) * src_w as i64) / fit.w as i64;
            let sy = ((y as i64) * src_h as i64) / fit.h as i64;
            let si = ((sy as u32 * src_w + sx as u32) * 4) as usize;
            if si + 3 >= src_premul.len() {
                continue;
            }
            let a = src_premul[si + 3] as u16;
            let dx = (fit.x + x) as u32;
            let dy = (fit.y + y) as u32;
            if dx >= dst_w || dy >= dst_h {
                continue;
            }
            let di = ((dy * dst_w + dx) * 4) as usize;
            for c in 0..3 {
                let s = src_premul[si + c] as u16;
                let b = bg[c] as u16;
                out[di + c] = (s + (b * (255 - a)) / 255).min(255) as u8;
            }
            out[di + 3] = 255;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_bitmap_is_letterboxed_vertically_and_centered() {
        let r = fit_centered(640, 400, 200, 200);
        assert_eq!(r.w, 200);
        assert_eq!(r.h, 125);
        assert_eq!(r.x, 0);
        assert!((r.y * 2 + r.h - 200).abs() <= 1);
    }

    #[test]
    fn tall_client_does_not_stretch() {
        let r = fit_centered(8, 5, 80, 100);
        assert_eq!((r.w, r.h), (80, 50));
        assert_eq!(r.y, 25);
    }

    #[test]
    fn opaque_bounds_ignore_an_interior_hole() {
        let mut px = vec![255u8; 4 * 4 * 4];
        let i = (1 * 4 + 1) * 4;
        px[i + 3] = 0;
        let (x0, y0, x1, y1) = opaque_bounds(4, 4, &px).unwrap();
        assert_eq!((x0, y0, x1, y1), (0, 0, 3, 3));
    }

    #[test]
    fn composite_centers_instead_of_pinning_to_the_origin() {
        let mut src = vec![0u8; 8 * 4 * 4];
        for px in src.chunks_exact_mut(4) {
            px[0] = 255;
            px[3] = 255;
        }
        let out = composite_centered(8, 4, &src, 20, 20, [0, 0, 0, 255]);
        let mut min_y = 20u32;
        let mut max_y = 0u32;
        for y in 0..20 {
            for x in 0..20 {
                let i = ((y * 20 + x) * 4) as usize;
                if out[i] == 255 {
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        assert!(min_y > 0, "bitmap pinned to the top/origin");
        assert!(max_y < 19);
        assert_eq!(out[0], 0);
    }
}
