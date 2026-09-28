//! RGBA (already premultiplied) → BGRA. Channel swap only; premultiply stays
//! in `dither-thumb` so it cannot be applied twice.

pub fn rgba_premul_to_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len() - rgba.len() % 4);
    for px in rgba.chunks_exact(4) {
        out.push(px[2]);
        out.push(px[1]);
        out.push(px[0]);
        out.push(px[3]);
    }
    out
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    rgba_premul_to_bgra(bgra)
}

/// Straight alpha for a PNG viewer. Identity for 0 and 255.
pub fn unpremultiply_rgba(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        let a = px[3] as u16;
        if a == 0 || a == 255 {
            out.extend_from_slice(px);
        } else {
            out.push(((px[0] as u16 * 255) / a).min(255) as u8);
            out.push(((px[1] as u16 * 255) / a).min(255) as u8);
            out.push(((px[2] as u16 * 255) / a).min(255) as u8);
            out.push(px[3]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swaps_r_and_b_and_keeps_premultiplied_values() {
        let bgra = rgba_premul_to_bgra(&[10, 20, 30, 128]);
        assert_eq!(bgra, vec![30, 20, 10, 128]);
    }

    #[test]
    fn unpremultiply_restores_half_blue() {
        let straight = unpremultiply_rgba(&[0, 0, 128, 128]);
        assert_eq!(straight, vec![0, 0, 255, 128]);
    }
}
