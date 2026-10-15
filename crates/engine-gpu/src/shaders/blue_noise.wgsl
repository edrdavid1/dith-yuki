// Threshold-shift blue noise — one thread per pixel, workgroup 8×8.
// Storage may be a strip: index as (y - buf_y0) * img_w + x.
// Spec: `.local-doc/Dither GPU-аналоги ED — схема реализации.md` §2.
//
// Option A (identity): palette α holds half nearest-neighbor Chebyshev distance
// max(|Δr|,|Δg|,|Δb|) / 2. |shift_q8| is clamped to (α << 8) for the closest
// palette entry to the *unshifted* pixel (Euclidean pick), so a solid palette
// color cannot leave its cell under the isotropic channel shift.

struct Params {
    img_w: u32,
    img_h: u32,
    palette_len: u32,
    spread: u32,
    noise_size: u32, // 128
    strip_y0: u32,
    strip_h: u32,
    buf_y0: u32,
}

@group(0) @binding(0) var<storage, read> input_px: array<u32>;
@group(0) @binding(1) var<storage, read_write> output_px: array<u32>;
@group(0) @binding(2) var<storage, read> palette: array<u32>;
@group(0) @binding(3) var<storage, read> noise: array<u32>; // packed u16 pairs
@group(0) @binding(4) var<uniform> params: Params;

fn unpack_r(p: u32) -> u32 { return p & 0xffu; }
fn unpack_g(p: u32) -> u32 { return (p >> 8u) & 0xffu; }
fn unpack_b(p: u32) -> u32 { return (p >> 16u) & 0xffu; }
fn unpack_a(p: u32) -> u32 { return p >> 24u; }

fn pack_rgba(r: u32, g: u32, b: u32, a: u32) -> u32 {
    return r | (g << 8u) | (b << 16u) | (a << 24u);
}

fn buf_idx(x: u32, y: u32) -> u32 {
    return (y - params.buf_y0) * params.img_w + x;
}

fn rank_at(x: u32, y: u32) -> u32 {
    let s = params.noise_size;
    let idx = (y % s) * s + (x % s);
    let word = noise[idx / 2u];
    if ((idx & 1u) == 0u) {
        return word & 0xffffu;
    }
    return word >> 16u;
}

fn nearest_idx(r: u32, g: u32, b: u32) -> u32 {
    let n = params.palette_len;
    var best_i = 0u;
    var best_d = 0x7fffffffu;
    for (var i = 0u; i < n; i++) {
        let p = palette[i];
        let dr = i32(r) - i32(unpack_r(p));
        let dg = i32(g) - i32(unpack_g(p));
        let db = i32(b) - i32(unpack_b(p));
        let d = dr * dr + dg * dg + db * db;
        if (u32(d) < best_d) {
            best_d = u32(d);
            best_i = i;
        }
    }
    return best_i;
}

fn nearest_rgb(r: u32, g: u32, b: u32) -> vec3<u32> {
    let best_i = nearest_idx(r, g, b);
    let p = palette[best_i];
    return vec3<u32>(unpack_r(p), unpack_g(p), unpack_b(p));
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = params.strip_y0 + gid.y;
    if (x >= params.img_w || y >= params.img_h || gid.y >= params.strip_h) {
        return;
    }

    let p = input_px[buf_idx(x, y)];
    let pr = unpack_r(p);
    let pg = unpack_g(p);
    let pb = unpack_b(p);

    let closest = nearest_idx(pr, pg, pb);
    let half = unpack_a(palette[closest]); // half NN dist (u8)
    let limit_q8 = i32(half) << 8;

    let rank = rank_at(x, y);
    // Fixed-point: t_q16 = rank * 4; shift_q8 = ((t_q16 - 32768) * spread) >> 8
    let t_q16 = i32(rank) * 4;
    var shift_q8 = ((t_q16 - 32768) * i32(params.spread)) >> 8;
    shift_q8 = clamp(shift_q8, -limit_q8, limit_q8);

    var out_c: array<u32, 3>;
    let chs = array<u32, 3>(pr, pg, pb);
    for (var i = 0u; i < 3u; i++) {
        let v_q8 = i32(chs[i]) << 8;
        let shifted = clamp(v_q8 + shift_q8, 0, 255 << 8);
        out_c[i] = u32((shifted + 128) >> 8);
    }
    let snap = nearest_rgb(out_c[0], out_c[1], out_c[2]);
    output_px[buf_idx(x, y)] = pack_rgba(snap.x, snap.y, snap.z, unpack_a(p));
}
