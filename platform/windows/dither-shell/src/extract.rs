//! `dither-thumb` call used by both COM classes. Kind comes from `mimetype`.

use crate::diag_codes::Reason;
use dither_thumb::{extract, ThumbError, ThumbKind};
use dither_zip_safe::io::ReadAt;
use dither_zip_safe::limits::ThumbLimits;

pub fn extract_preview(
    io: &mut dyn ReadAt,
    max_side: u32,
    premultiply: bool,
    limits: &ThumbLimits,
) -> Result<dither_thumb::ThumbBitmap, Reason> {
    if max_side == 0 {
        return Err(Reason::InvalidArg);
    }
    match extract(io, ThumbKind::Project, max_side, premultiply, limits) {
        Ok(bmp) => Ok(bmp),
        Err(ThumbError::NotAvailable) => {
            extract(io, ThumbKind::Pattern, max_side, premultiply, limits).map_err(map_err)
        }
        Err(e) => Err(map_err(e)),
    }
}

fn map_err(err: ThumbError) -> Reason {
    match err {
        ThumbError::NotAvailable => Reason::NoThumbnail,
        ThumbError::Limit => Reason::Limit,
        ThumbError::Corrupt | ThumbError::Unsupported => Reason::Corrupt,
        ThumbError::Timeout => Reason::Timeout,
        ThumbError::BadArg => Reason::InvalidArg,
        ThumbError::Internal => Reason::Internal,
    }
}
