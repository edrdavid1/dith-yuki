// Ordered two-nearest Oklab snap — Mixed pass 2 (Path B).
// Reads Guided RGB from scratch; recomputes Bayer threshold for pick.

const HALO: u32 = 2u;
const CORE: u32 = 256u;

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct SnapUniforms {
    tile: TileUniforms,
    // x=threshold_scale, y=palette_len (z/w unused)
    params: vec4<f32>,
}

struct BayerPatternUniforms {
    // x=pattern_sin, y=pattern_cos, z=unused, w=threshold_bias
    packed: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: SnapUniforms;
@group(0) @binding(1) var tile_in: texture_2d<f32>;
@group(0) @binding(2) var tile_out: texture_storage_2d<rgba32float, write>;
@group(0) @binding(3) var<storage, read> pat: BayerPatternUniforms;
@group(0) @binding(4) var<storage, read> palette_rgb: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read> palette_lab: array<vec4<f32>>;

const BAYER2: array<u32, 4> = array<u32, 4>(0u, 2u, 3u, 1u);
const BAYER4: array<u32, 16> = array<u32, 16>(
    0u, 8u, 2u, 10u,
    12u, 4u, 14u, 6u,
    3u, 11u, 1u, 9u,
    15u, 7u, 13u, 5u
);
const BAYER8: array<u32, 64> = array<u32, 64>(
    0u, 32u, 8u, 40u, 2u, 34u, 10u, 42u,
    48u, 16u, 56u, 24u, 50u, 18u, 58u, 26u,
    12u, 44u, 4u, 36u, 14u, 46u, 6u, 38u,
    60u, 28u, 52u, 20u, 62u, 30u, 54u, 22u,
    3u, 35u, 11u, 43u, 1u, 33u, 9u, 41u,
    51u, 19u, 59u, 27u, 49u, 17u, 57u, 25u,
    15u, 47u, 7u, 39u, 13u, 45u, 5u, 37u,
    63u, 31u, 55u, 23u, 61u, 29u, 53u, 21u
);

fn rem_euclid_i32(a: i32, n: i32) -> i32 {
    let r = a % n;
    if (r < 0) {
        return r + n;
    }
    return r;
}

const ROT_FLOOR_BIAS: f32 = 5e-6;

fn rotate_with_trig(gx: i32, gy: i32, sin_t: f32, cos_t: f32) -> vec2<i32> {
    if (abs(sin_t) < 0.0001) {
        return vec2<i32>(gx, gy);
    }
    let x = f32(gx);
    let y = f32(gy);
    let xr = x * cos_t - y * sin_t;
    let yr = x * sin_t + y * cos_t;
    return vec2<i32>(
        i32(floor(xr - ROT_FLOOR_BIAS)),
        i32(floor(yr - ROT_FLOOR_BIAS)),
    );
}

fn apply_threshold_bias(threshold: f32, bias: f32) -> f32 {
    if (bias == 0.0) {
        return threshold;
    }
    return clamp(threshold + bias, 0.0, 0.999999);
}

fn bayer2_i32(gx: i32, gy: i32) -> f32 {
    let mx = rem_euclid_i32(gx, 2);
    let my = rem_euclid_i32(gy, 2);
    return f32(BAYER2[my * 2 + mx]) / 4.0;
}

fn bayer4_i32(gx: i32, gy: i32) -> f32 {
    let mx = rem_euclid_i32(gx, 4);
    let my = rem_euclid_i32(gy, 4);
    return f32(BAYER4[my * 4 + mx]) / 16.0;
}

fn bayer8_i32(gx: i32, gy: i32) -> f32 {
    let mx = rem_euclid_i32(gx, 8);
    let my = rem_euclid_i32(gy, 8);
    return f32(BAYER8[my * 8 + mx]) / 64.0;
}

fn sample_bayer(gx: i32, gy: i32, matrix: u32) -> f32 {
    let rot = rotate_with_trig(gx, gy, pat.packed.x, pat.packed.y);
    var t = 0.0;
    switch (matrix) {
        case 2u: { t = bayer2_i32(rot.x, rot.y); }
        case 4u: { t = bayer4_i32(rot.x, rot.y); }
        default: { t = bayer8_i32(rot.x, rot.y); }
    }
    return apply_threshold_bias(t, pat.packed.w);
}

fn linear_to_oklab(rgb: vec3<f32>) -> vec3<f32> {
    let r = clamp(rgb.r, 0.0, 1.0);
    let g = clamp(rgb.g, 0.0, 1.0);
    let b = clamp(rgb.b, 0.0, 1.0);
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
    let l_ = pow(l, 1.0 / 3.0);
    let m_ = pow(m, 1.0 / 3.0);
    let s_ = pow(s, 1.0 / 3.0);
    return vec3<f32>(
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    );
}

fn oklab_dist_sq(a: vec3<f32>, b: vec3<f32>) -> f32 {
    let d = a - b;
    return dot(d, d);
}

fn ordered_two_nearest(rgb: vec3<f32>, threshold: f32, threshold_scale: f32) -> vec3<f32> {
    let n = u32(u.params.y);
    if (n == 0u) {
        return rgb;
    }
    if (n == 1u) {
        return palette_rgb[0].xyz;
    }

    let query = linear_to_oklab(rgb);
    var i1 = 0u;
    var i2 = 1u;
    var d1 = oklab_dist_sq(query, palette_lab[0].xyz);
    var d2 = oklab_dist_sq(query, palette_lab[1].xyz);
    if (d2 < d1) {
        let ti = i1; i1 = i2; i2 = ti;
        let td = d1; d1 = d2; d2 = td;
    }
    for (var i = 2u; i < n; i++) {
        let d = oklab_dist_sq(query, palette_lab[i].xyz);
        if (d < d1) {
            d2 = d1;
            i2 = i1;
            d1 = d;
            i1 = i;
        } else if (d < d2) {
            d2 = d;
            i2 = i;
        }
    }

    var mix = 0.0;
    if (d1 + d2 > 1e-20) {
        let sd1 = sqrt(d1);
        let sd2 = sqrt(d2);
        mix = sd1 / (sd1 + sd2);
    }
    let t = 0.5 + (threshold - 0.5) * threshold_scale;
    let idx = select(i1, i2, t < mix);
    return palette_rgb[idx].xyz;
}

fn snap_at(gid: vec3<u32>, threshold: f32) {
    if (gid.x >= CORE || gid.y >= CORE) {
        return;
    }
    let px = i32(gid.x + HALO);
    let py = i32(gid.y + HALO);
    let rgba = textureLoad(tile_in, vec2<i32>(px, py), 0);
    let snapped = ordered_two_nearest(rgba.rgb, threshold, u.params.x);
    textureStore(tile_out, vec2<i32>(px, py), vec4<f32>(snapped, rgba.a));
}

@compute @workgroup_size(16, 16)
fn bayer2_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    snap_at(gid, sample_bayer(gx, gy, 2u));
}

@compute @workgroup_size(16, 16)
fn bayer4_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    snap_at(gid, sample_bayer(gx, gy, 4u));
}

@compute @workgroup_size(16, 16)
fn bayer8_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    snap_at(gid, sample_bayer(gx, gy, 8u));
}
