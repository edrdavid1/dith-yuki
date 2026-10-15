// Blocked Floyd–Steinberg — wavefront t = x + 2y inside one workgroup.
// Geometry A: 56×56 core + 8px left/top warm-up = 64×64 (unchanged).
//
// Memory C: no full tile / err plane in shared memory.
//   - Input pixels read from storage (edge-clamped).
//   - Error: ring of 4 slots × 3 channels × 66 rows (atomic i32).
//
// Ring proof (t = x + 2y): destination at t_d is written from t_d−1 (e7,e3),
// t_d−2 (e5), t_d−3 (e1). Contribution lifetime ∈ {1,2,3} waves. At wave t,
// slots for t..t+3 may be live → ring ≥ 4. Ring 3 aliases t+3 ≡ t (mod 3).
//
// Same-wave races: e7 from (x,y) and e3 from (x+2,y−1) both target (x+1,y) at
// t+1 → atomics on ring cells.
//
// Every wave clears ring[t mod 4][y] for all y (even if x out of range) so
// out-of-bounds e7/e1 cannot poison the slot on reuse at t+4.
//
// Joint nearest: corrected RGB (all three channels + their errors) → one palette
// pick (Euclidean display-u8, lower index wins). Per-channel substitution removed.

struct Params {
    img_w: u32,
    img_h: u32,
    core: u32,
    pad: u32,
    palette_len: u32,
    strip_y0: u32,
    buf_y0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<storage, read> input_px: array<u32>;
@group(0) @binding(1) var<storage, read_write> output_px: array<u32>;
@group(0) @binding(2) var<storage, read> palette: array<u32>;
@group(0) @binding(3) var<uniform> params: Params;

const WG: u32 = 64u;
const RING: u32 = 4u;
const ROWS: u32 = 66u; // WG + 2
// Shared: 3 * 4 * 66 * 4 = 3168 bytes (≪ 32768).
var<workgroup> err_ring: array<atomic<i32>, 792>;

fn ring_at(ch: u32, slot: u32, y: i32) -> u32 {
    let row = u32(y + 1);
    return (ch * RING + (slot % RING)) * ROWS + row;
}

fn unpack_r(p: u32) -> u32 { return p & 0xffu; }
fn unpack_g(p: u32) -> u32 { return (p >> 8u) & 0xffu; }
fn unpack_b(p: u32) -> u32 { return (p >> 16u) & 0xffu; }
fn unpack_a(p: u32) -> u32 { return p >> 24u; }

fn pack_rgba(r: u32, g: u32, b: u32, a: u32) -> u32 {
    return r | (g << 8u) | (b << 16u) | (a << 24u);
}

fn clamp_xy(x: i32, y: i32) -> vec2<u32> {
    let cx = clamp(x, 0, i32(params.img_w) - 1);
    let cy = clamp(y, 0, i32(params.img_h) - 1);
    return vec2<u32>(u32(cx), u32(cy));
}

fn buf_idx(x: u32, y: u32) -> u32 {
    return (y - params.buf_y0) * params.img_w + x;
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
        // Strict < keeps lower index on ties.
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

    for (var i = li; i < 792u; i += 64u) {
        atomicStore(&err_ring[i], 0);
    }
    workgroupBarrier();

    let t_max = (WG - 1u) + 2u * (WG - 1u);
    for (var t = 0u; t <= t_max; t++) {
        let slot = t % RING;
        // Always consume+clear this row's current slot (poison defense for t+4 reuse).
        let er = atomicLoad(&err_ring[ring_at(0u, slot, i32(y))]);
        let eg = atomicLoad(&err_ring[ring_at(1u, slot, i32(y))]);
        let eb = atomicLoad(&err_ring[ring_at(2u, slot, i32(y))]);
        atomicStore(&err_ring[ring_at(0u, slot, i32(y))], 0);
        atomicStore(&err_ring[ring_at(1u, slot, i32(y))], 0);
        atomicStore(&err_ring[ring_at(2u, slot, i32(y))], 0);

        let x_i = i32(t) - 2 * i32(y);
        var do_px = false;
        var x: u32 = 0u;
        var clamped_r: i32 = 0;
        var clamped_g: i32 = 0;
        var clamped_b: i32 = 0;
        var pix: u32 = 0u;

        if (x_i >= 0 && x_i < i32(WG)) {
            do_px = true;
            x = u32(x_i);
            let gx = i32(tile_x) - i32(pad) + i32(x);
            let gy = i32(tile_y) - i32(pad) + i32(y);
            let c = clamp_xy(gx, gy);
            pix = input_px[buf_idx(c.x, c.y)];

            clamped_r = clamp((i32(unpack_r(pix)) << 8) + er, 0, 255 << 8);
            clamped_g = clamp((i32(unpack_g(pix)) << 8) + eg, 0, 255 << 8);
            clamped_b = clamp((i32(unpack_b(pix)) << 8) + eb, 0, 255 << 8);
        }
        workgroupBarrier();

        if (do_px) {
            let cr = u32((clamped_r + 128) >> 8);
            let cg = u32((clamped_g + 128) >> 8);
            let cb = u32((clamped_b + 128) >> 8);
            let snap = nearest_rgb(cr, cg, cb);

            let errs = array<i32, 3>(
                clamped_r - (i32(snap.x) << 8),
                clamped_g - (i32(snap.y) << 8),
                clamped_b - (i32(snap.z) << 8),
            );
            for (var ch = 0u; ch < 3u; ch++) {
                let error = errs[ch];
                let e7 = (error * 7) >> 4;
                let e3 = (error * 3) >> 4;
                let e5 = (error * 5) >> 4;
                let e1 = error - e7 - e3 - e5;
                // Skip writes whose destination x is outside the WG (no clear consumer).
                if (x + 1u < WG) {
                    atomicAdd(&err_ring[ring_at(ch, t + 1u, i32(y))], e7);
                }
                if (x > 0u) {
                    atomicAdd(&err_ring[ring_at(ch, t + 1u, i32(y) + 1)], e3);
                }
                atomicAdd(&err_ring[ring_at(ch, t + 2u, i32(y) + 1)], e5);
                if (x + 1u < WG) {
                    atomicAdd(&err_ring[ring_at(ch, t + 3u, i32(y) + 1)], e1);
                }
            }

            if (y >= pad && y < pad + core && x >= pad && x < pad + core) {
                let gx = tile_x + (x - pad);
                let gy = tile_y + (y - pad);
                if (gx < params.img_w && gy < params.img_h) {
                    output_px[buf_idx(gx, gy)] = pack_rgba(snap.x, snap.y, snap.z, unpack_a(pix));
                }
            }
        }
        workgroupBarrier();
    }
}
