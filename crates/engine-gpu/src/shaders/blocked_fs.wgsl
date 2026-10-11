// Blocked Floyd–Steinberg — wavefront t = x + 2y inside one workgroup.
// Geometry: 56×56 core + 8px left/top warm-up = 64×64.
// Storage may be a strip: index as (y - buf_y0) * img_w + x.
// Spec: `.local-doc/Dither GPU-аналоги ED — схема реализации.md` §1.

struct Params {
    img_w: u32,
    img_h: u32,
    core: u32,
    pad: u32,
    palette_len: u32,
    strip_y0: u32, // absolute Y of first core row in this dispatch
    buf_y0: u32,   // absolute Y of row 0 in the bound STORAGE arrays
    _pad1: u32,
}

@group(0) @binding(0) var<storage, read> input_px: array<u32>;
@group(0) @binding(1) var<storage, read_write> output_px: array<u32>;
@group(0) @binding(2) var<storage, read> palette: array<u32>;
@group(0) @binding(3) var<uniform> params: Params;

const WG: u32 = 64u;

var<workgroup> tile: array<u32, 4096>; // packed RGBA8
// atomics: same-wave writers can hit one cell (e7 from (x,y) and e3 from (x+2,y−1)).
var<workgroup> err: array<atomic<i32>, 4356>; // 66×66 Q8.8

fn err_at(x: i32, y: i32) -> u32 {
    return u32(y + 1) * 66u + u32(x + 1);
}

fn unpack_r(p: u32) -> u32 { return p & 0xffu; }
fn unpack_g(p: u32) -> u32 { return (p >> 8u) & 0xffu; }
fn unpack_b(p: u32) -> u32 { return (p >> 16u) & 0xffu; }
fn unpack_a(p: u32) -> u32 { return p >> 24u; }

fn pack_rgba(r: u32, g: u32, b: u32, a: u32) -> u32 {
    return r | (g << 8u) | (b << 16u) | (a << 24u);
}

fn set_ch(p: u32, ch: u32, v: u32) -> u32 {
    let mask = 0xffu << (ch * 8u);
    return (p & ~mask) | ((v & 0xffu) << (ch * 8u));
}

fn get_ch(p: u32, ch: u32) -> u32 {
    return (p >> (ch * 8u)) & 0xffu;
}

fn clamp_xy(x: i32, y: i32) -> vec2<u32> {
    let cx = clamp(x, 0, i32(params.img_w) - 1);
    let cy = clamp(y, 0, i32(params.img_h) - 1);
    return vec2<u32>(u32(cx), u32(cy));
}

fn buf_idx(x: u32, y: u32) -> u32 {
    return (y - params.buf_y0) * params.img_w + x;
}

/// Nearest palette entry for probe RGB; return channel `ch` of that entry.
/// Matches CPU: replace active channel with `color`, keep other channels.
fn nearest_ch(pix: u32, color: u32, ch: u32) -> u32 {
    var pr = unpack_r(pix);
    var pg = unpack_g(pix);
    var pb = unpack_b(pix);
    if (ch == 0u) { pr = color; }
    else if (ch == 1u) { pg = color; }
    else { pb = color; }

    let n = params.palette_len;
    var best_i = 0u;
    var best_d = 0x7fffffffu;
    for (var i = 0u; i < n; i++) {
        let p = palette[i];
        let dr = i32(pr) - i32(unpack_r(p));
        let dg = i32(pg) - i32(unpack_g(p));
        let db = i32(pb) - i32(unpack_b(p));
        let d = dr * dr + dg * dg + db * db;
        if (u32(d) < best_d) {
            best_d = u32(d);
            best_i = i;
        }
    }
    return get_ch(palette[best_i], ch);
}

fn nearest_rgb(r: u32, g: u32, b: u32) -> vec3<u32> {
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
    let p = palette[best_i];
    return vec3<u32>(unpack_r(p), unpack_g(p), unpack_b(p));
}

@compute @workgroup_size(64)
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(local_invocation_index) li: u32,
) {
    let core = params.core;
    let pad = params.pad;
    let tile_x = wid.x * core;
    let tile_y = params.strip_y0 + wid.y * core;
    let y = li;

    // Load padded 64×64 (edge-clamped).
    for (var x = 0u; x < WG; x++) {
        let gx = i32(tile_x) - i32(pad) + i32(x);
        let gy = i32(tile_y) - i32(pad) + i32(y);
        let c = clamp_xy(gx, gy);
        tile[y * WG + x] = input_px[buf_idx(c.x, c.y)];
    }
    workgroupBarrier();

    // R, G, B sequential — reuse err plane.
    for (var ch = 0u; ch < 3u; ch++) {
        for (var i = li; i < 4356u; i += 64u) {
            atomicStore(&err[i], 0);
        }
        workgroupBarrier();

        let t_max = (WG - 1u) + 2u * (WG - 1u);
        for (var t = 0u; t <= t_max; t++) {
            let x_i = i32(t) - 2 * i32(y);
            if (x_i >= 0 && x_i < i32(WG)) {
                let x = u32(x_i);
                let idx = y * WG + x;
                let pix = tile[idx];
                let v_q8 = i32(get_ch(pix, ch)) << 8;
                let adjusted = v_q8 + atomicLoad(&err[err_at(i32(x), i32(y))]);
                let clamped = clamp(adjusted, 0, 255 << 8);
                let color_u8 = u32((clamped + 128) >> 8);
                let quantized = nearest_ch(pix, color_u8, ch);
                let error = clamped - (i32(quantized) << 8);

                let e7 = (error * 7) >> 4;
                let e3 = (error * 3) >> 4;
                let e5 = (error * 5) >> 4;
                let e1 = error - e7 - e3 - e5;

                atomicAdd(&err[err_at(i32(x) + 1, i32(y))], e7);
                atomicAdd(&err[err_at(i32(x) - 1, i32(y) + 1)], e3);
                atomicAdd(&err[err_at(i32(x), i32(y) + 1)], e5);
                atomicAdd(&err[err_at(i32(x) + 1, i32(y) + 1)], e1);

                tile[idx] = set_ch(pix, ch, quantized);
            }
            workgroupBarrier();
        }
        workgroupBarrier();
    }

    // Joint palette snap + write core.
    for (var x = 0u; x < WG; x++) {
        let idx = y * WG + x;
        let pix = tile[idx];
        let snap = nearest_rgb(unpack_r(pix), unpack_g(pix), unpack_b(pix));
        tile[idx] = pack_rgba(snap.x, snap.y, snap.z, unpack_a(pix));
    }
    workgroupBarrier();

    if (y < core) {
        let gy = tile_y + y;
        if (gy < params.img_h) {
            for (var x = 0u; x < core; x++) {
                let gx = tile_x + x;
                if (gx < params.img_w) {
                    let sx = pad + x;
                    let sy = pad + y;
                    output_px[buf_idx(gx, gy)] = tile[sy * WG + sx];
                }
            }
        }
    }
}
