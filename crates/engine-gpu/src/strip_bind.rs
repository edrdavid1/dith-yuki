//! Strip height / binding-size helpers for GPU dither families.

/// Must match [`crate::blocked_fs::PAD`].
pub const PAD: u32 = 8;
/// Must match [`crate::blocked_fs::CORE`].
pub const CORE: u32 = 56;

/// Effective max STORAGE binding size (device limit, optionally capped for tests).
pub fn effective_max_bind(device: &wgpu::Device, force_max_bind: Option<u64>) -> u64 {
    let lim = device.limits().max_storage_buffer_binding_size as u64;
    match force_max_bind {
        Some(v) => v.min(lim).max(1),
        None => lim,
    }
}

/// Bytes per image row (RGBA8).
pub fn row_bytes(width: u32) -> u64 {
    width as u64 * 4
}

/// Max core-rows per blocked_fs strip given binding budget.
/// Reserves [`PAD`] rows above the cores for warm-up.
/// Returns `None` if even one core-row cannot fit.
pub fn max_blocked_fs_strip_cores(width: u32, max_bind: u64) -> Option<u32> {
    let rb = row_bytes(width);
    if rb == 0 || rb > max_bind {
        return None;
    }
    let max_rows = max_bind / rb;
    // Always reserve PAD for warm-up (conservative; first strip may use less).
    let core_px = max_rows.saturating_sub(PAD as u64);
    let cores = core_px / CORE as u64;
    if cores == 0 {
        return None;
    }
    Some(cores as u32)
}

/// Absolute row range covered by a blocked_fs strip (includes warm-up above).
pub fn blocked_fs_strip_rows(
    strip_y0: u32,
    strip_cores: u32,
    height: u32,
) -> (u32, u32) {
    let y0 = strip_y0.saturating_sub(PAD);
    let y1 = (strip_y0 + strip_cores * CORE).min(height);
    (y0, y1)
}

/// Max pixel rows per blue_noise strip.
pub fn max_blue_noise_strip_h(width: u32, max_bind: u64) -> Option<u32> {
    let rb = row_bytes(width);
    if rb == 0 || rb > max_bind {
        return None;
    }
    let h = (max_bind / rb) as u32;
    if h == 0 {
        None
    } else {
        Some(h)
    }
}

/// Align `offset` down to `align` (must be power of two).
/// Used when binding a sub-range of a large buffer (`BufferBinding::{offset,size}`).
#[allow(dead_code)]
pub fn align_down(offset: u64, align: u64) -> u64 {
    if align <= 1 {
        return offset;
    }
    offset & !(align - 1)
}

/// Given a desired start row, find the largest `buf_y0 <= y0` such that
/// `buf_y0 * row_bytes` is a multiple of `min_storage_buffer_offset_alignment`.
#[allow(dead_code)]
pub fn aligned_buf_y0(y0: u32, width: u32, align: u32) -> Option<u32> {
    let rb = row_bytes(width);
    let align = align.max(1) as u64;
    if rb % align == 0 {
        return Some(y0);
    }
    let mut y = y0 as u64;
    loop {
        if (y * rb) % align == 0 {
            return Some(y as u32);
        }
        if y == 0 {
            return None;
        }
        y -= 1;
    }
}
