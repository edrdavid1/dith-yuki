//! Geometry report for `render`. PNG encoding lives in `dither-shell-diag`
//! so this crate does not take an image dependency.

use crate::geometry::opaque_bounds;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderReport {
    pub width: u32,
    pub height: u32,
    pub opaque_bbox_full: bool,
    pub via_hbitmap: bool,
}

pub fn report(width: u32, height: u32, premul_rgba: &[u8], via_hbitmap: bool) -> RenderReport {
    let full = opaque_bounds(width, height, premul_rgba)
        .is_some_and(|(x0, y0, x1, y1)| x0 == 0 && y0 == 0 && x1 + 1 == width && y1 + 1 == height);
    RenderReport {
        width,
        height,
        opaque_bbox_full: full,
        via_hbitmap,
    }
}

pub fn format_report(r: &RenderReport) -> String {
    format!(
        "size {}x{}\nopaque_bbox_full {}\nhbitmap {}\n",
        r.width,
        r.height,
        if r.opaque_bbox_full { "PASS" } else { "FAIL" },
        if r.via_hbitmap {
            "PASS"
        } else {
            "SKIP (Windows HBITMAP path not used)"
        },
    )
}
