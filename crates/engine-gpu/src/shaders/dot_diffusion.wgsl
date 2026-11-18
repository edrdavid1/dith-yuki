// Knuth dot diffusion — one class (0..63) per dispatch.
// Each thread owns one 8×8 block and writes the pixel of the current class.
// Same-class pixels are 8 apart, so their 3×3 neighborhoods do not overlap.
// Host runs classes in order and keeps `err` across dispatches.
//
// Orthogonal neighbors weigh 2, diagonals weigh 1. Incoming error from an
// already-processed neighbor N is `err(N) * w(N→P) / W(N)`, Q8.8, truncating
// toward zero. W(N) is the sum of N's edges toward higher classes still in frame.

struct Params {
    img_w: u32,
    img_h: u32,
    palette_len: u32,
    cls: u32,
    cls_x: u32,
    cls_y: u32,
    _p0: u32,
    _p1: u32,
}

@group(0) @binding(0) var<storage, read> input_px: array<u32>;
@group(0) @binding(1) var<storage, read_write> output_px: array<u32>;
@group(0) @binding(2) var<storage, read> palette: array<u32>;
@group(0) @binding(4) var<uniform> params: Params;
@group(0) @binding(5) var<storage, read_write> err: array<i32>;

var<private> CLASS: array<u32, 64> = array<u32, 64>(
    // @class-matrix
    34u, 48u, 40u, 32u, 29u, 15u, 23u, 31u,
    42u, 58u, 56u, 53u, 21u, 5u, 7u, 10u,
    50u, 62u, 61u, 45u, 13u, 1u, 2u, 18u,
    38u, 46u, 54u, 37u, 25u, 17u, 9u, 26u,
    28u, 14u, 22u, 30u, 35u, 49u, 41u, 33u,
    20u, 4u, 6u, 11u, 43u, 59u, 57u, 52u,
    12u, 0u, 3u, 19u, 51u, 63u, 60u, 44u,
    24u, 16u, 8u, 27u, 39u, 47u, 55u, 36u,
);

fn class_of(x: i32, y: i32) -> u32 {
    return CLASS[u32(y & 7) * 8u + u32(x & 7)];
}

fn in_img(x: i32, y: i32) -> bool {
    return x >= 0 && y >= 0 && x < i32(params.img_w) && y < i32(params.img_h);
}

fn wgt(dx: i32, dy: i32) -> i32 {
    return select(1, 2, dx == 0 || dy == 0);
}

fn out_weight(nx: i32, ny: i32) -> i32 {
    let cn = class_of(nx, ny);
    var s = 0;
    for (var dy: i32 = -1; dy <= 1; dy++) {
        for (var dx: i32 = -1; dx <= 1; dx++) {
            if ((dx == 0 && dy == 0) || !in_img(nx + dx, ny + dy)) {
                continue;
            }
            if (class_of(nx + dx, ny + dy) > cn) {
                s += wgt(dx, dy);
            }
        }
    }
    return s;
}

fn unpack_r(p: u32) -> u32 { return p & 0xffu; }
fn unpack_g(p: u32) -> u32 { return (p >> 8u) & 0xffu; }
fn unpack_b(p: u32) -> u32 { return (p >> 16u) & 0xffu; }
fn unpack_a(p: u32) -> u32 { return p >> 24u; }

fn pack_rgba(r: u32, g: u32, b: u32, a: u32) -> u32 {
    return r | (g << 8u) | (b << 16u) | (a << 24u);
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
    let xi = i32(gid.x * 8u + params.cls_x);
    let yi = i32(gid.y * 8u + params.cls_y);
    if (!in_img(xi, yi)) {
        return;
    }
    let idx = u32(yi) * params.img_w + u32(xi);

    var acc = vec3<i32>(0, 0, 0);
    for (var dy: i32 = -1; dy <= 1; dy++) {
        for (var dx: i32 = -1; dx <= 1; dx++) {
            let nx = xi + dx;
            let ny = yi + dy;
            if ((dx == 0 && dy == 0) || !in_img(nx, ny)) {
                continue;
            }
            if (class_of(nx, ny) >= params.cls) {
                continue;
            }
            let big_w = out_weight(nx, ny);
            let w = wgt(-dx, -dy);
            let ni = (u32(ny) * params.img_w + u32(nx)) * 3u;
            acc += vec3<i32>(err[ni], err[ni + 1u], err[ni + 2u]) * w / big_w;
        }
    }

    let p = input_px[idx];
    let src = vec3<i32>(i32(unpack_r(p)), i32(unpack_g(p)), i32(unpack_b(p)));
    let v = clamp((src << vec3<u32>(8u)) + acc, vec3<i32>(0, 0, 0), vec3<i32>(65280, 65280, 65280));
    let c = vec3<u32>(u32((v.x + 128) >> 8), u32((v.y + 128) >> 8), u32((v.z + 128) >> 8));
    let snap = nearest_rgb(c.x, c.y, c.z);

    let e = v - (vec3<i32>(i32(snap.x), i32(snap.y), i32(snap.z)) << vec3<u32>(8u));
    let ei = idx * 3u;
    err[ei] = e.x;
    err[ei + 1u] = e.y;
    err[ei + 2u] = e.z;
    output_px[idx] = pack_rgba(snap.x, snap.y, snap.z, unpack_a(p));
}
