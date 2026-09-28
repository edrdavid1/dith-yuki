//! File → bitmap used by `dither-shell-diag render`.
//! On Windows the pixels are read back from the HBITMAP.

use crate::diag_codes::{record, Reason};
use crate::extract::extract_preview;
use crate::file_io::FileAt;
use crate::png_out::{format_report, report, RenderReport};
use dither_zip_safe::io::ReadAt;
use dither_zip_safe::limits::ThumbLimits;
use std::path::Path;
use std::time::{Duration, Instant};

pub fn render_file(path: &Path, max_side: u32) -> Result<(RenderReport, Vec<u8>), String> {
    if max_side == 0 {
        record(Reason::InvalidArg);
        return Err("INVALID_ARG".into());
    }
    let mut io = FileAt::open(path).map_err(|_| {
        record(Reason::InitFailed);
        "INIT_FAILED".to_string()
    })?;
    let limits = ThumbLimits::default();
    if io.size() == 0 || io.size() > limits.max_archive_dyproj.max(limits.max_archive_dyuki) {
        record(Reason::Limit);
        return Err("LIMIT".into());
    }
    let deadline = Instant::now() + Duration::from_millis(limits.deadline_ms);
    let _ = deadline;
    let bmp = extract_preview(&mut io, max_side, true, &limits).map_err(|reason| {
        record(reason);
        reason.as_str().to_string()
    })?;

    #[cfg(windows)]
    {
        use crate::color::rgba_premul_to_bgra;
        use crate::gdi::{create_bgra_premul_dib, delete_bitmap, read_premul_rgba};
        let bgra = rgba_premul_to_bgra(&bmp.rgba);
        let hbmp =
            create_bgra_premul_dib(bmp.width as i32, bmp.height as i32, &bgra).map_err(|_| {
                record(Reason::BitmapCreateFailed);
                "BITMAP_CREATE_FAILED".to_string()
            })?;
        let read = read_premul_rgba(hbmp);
        delete_bitmap(hbmp);
        let (w, h, px) = read.map_err(|_| {
            record(Reason::BitmapCreateFailed);
            "BITMAP_CREATE_FAILED".to_string()
        })?;
        let report = report(w, h, &px, true);
        return Ok((report, px));
    }

    #[cfg(not(windows))]
    {
        let report = report(bmp.width, bmp.height, &bmp.rgba, false);
        Ok((report, bmp.rgba))
    }
}

pub fn render_text(report: &RenderReport) -> String {
    format_report(report)
}
