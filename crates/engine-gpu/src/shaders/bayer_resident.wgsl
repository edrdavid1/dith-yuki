// Bayer on GPU-resident storage textures — separate read/write bindings (baseline WebGPU).

const HALO: u32 = 2u;
const CORE: u32 = 256u;

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct BayerUniforms {
    tile: TileUniforms,
    // x=levels, y=threshold_scale (z/w unused — keep primary uniform ≤32 B on Metal)
    params: vec4<f32>,
}

struct BayerPatternUniforms {
    // x=pattern_sin, y=pattern_cos, z=color_mode, w=threshold_bias
    packed: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: BayerUniforms;
@group(0) @binding(1) var tile_in: texture_2d<f32>;
@group(0) @binding(2) var tile_out: texture_storage_2d<rgba32float, write>;
@group(0) @binding(3) var<storage, read> pat: BayerPatternUniforms;

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
const BAYER16: array<u32, 256> = array<u32, 256>(
    0u, 128u, 32u, 160u, 8u, 136u, 40u, 168u, 2u, 130u, 34u, 162u, 10u, 138u, 42u, 170u,
    192u, 64u, 224u, 96u, 200u, 72u, 232u, 104u, 194u, 66u, 226u, 98u, 202u, 74u, 234u, 106u,
    48u, 176u, 16u, 144u, 56u, 184u, 24u, 152u, 50u, 178u, 18u, 146u, 58u, 186u, 26u, 154u,
    240u, 112u, 208u, 80u, 248u, 120u, 216u, 88u, 242u, 114u, 210u, 82u, 250u, 122u, 218u, 90u,
    12u, 140u, 44u, 172u, 4u, 132u, 36u, 164u, 14u, 142u, 46u, 174u, 6u, 134u, 38u, 166u,
    204u, 76u, 236u, 108u, 196u, 68u, 228u, 100u, 206u, 78u, 238u, 110u, 198u, 70u, 230u, 102u,
    60u, 188u, 28u, 156u, 52u, 180u, 20u, 148u, 62u, 190u, 30u, 158u, 54u, 182u, 22u, 150u,
    252u, 124u, 220u, 92u, 244u, 116u, 212u, 84u, 254u, 126u, 222u, 94u, 246u, 118u, 214u, 86u,
    3u, 131u, 35u, 163u, 11u, 139u, 43u, 171u, 1u, 129u, 33u, 161u, 9u, 137u, 41u, 169u,
    195u, 67u, 227u, 99u, 203u, 75u, 235u, 107u, 193u, 65u, 225u, 97u, 201u, 73u, 233u, 105u,
    51u, 179u, 19u, 147u, 59u, 187u, 27u, 155u, 49u, 177u, 17u, 145u, 57u, 185u, 25u, 153u,
    243u, 115u, 211u, 83u, 251u, 123u, 219u, 91u, 241u, 113u, 209u, 81u, 249u, 121u, 217u, 89u,
    15u, 143u, 47u, 175u, 7u, 135u, 39u, 167u, 13u, 141u, 45u, 173u, 5u, 133u, 37u, 165u,
    207u, 79u, 239u, 111u, 199u, 71u, 231u, 103u, 205u, 77u, 237u, 109u, 197u, 69u, 229u, 101u,
    63u, 191u, 31u, 159u, 55u, 183u, 23u, 151u, 61u, 189u, 29u, 157u, 53u, 181u, 21u, 149u,
    255u, 127u, 223u, 95u, 247u, 119u, 215u, 87u, 253u, 125u, 221u, 93u, 245u, 117u, 213u, 85u
);
// Classical clustered-dot 8×8 (newspaper 45°); ranks 0..63.
const CLUSTERED8: array<u32, 64> = array<u32, 64>(
    24u, 10u, 12u, 26u, 35u, 47u, 49u, 37u,
    8u, 0u, 2u, 14u, 45u, 59u, 61u, 51u,
    22u, 6u, 4u, 16u, 43u, 57u, 63u, 53u,
    30u, 20u, 18u, 28u, 33u, 41u, 55u, 39u,
    34u, 46u, 48u, 36u, 25u, 11u, 13u, 27u,
    44u, 58u, 60u, 50u, 9u, 1u, 3u, 15u,
    42u, 56u, 62u, 52u, 23u, 7u, 5u, 17u,
    32u, 40u, 54u, 38u, 31u, 21u, 19u, 29u
);
// Ulichney dispersed-dot 16×16; ranks 0..255 (not Bayer-16).
const DISPERSED16: array<u32, 256> = array<u32, 256>(
    1u, 235u, 59u, 219u, 15u, 231u, 55u, 215u, 2u, 232u, 56u, 216u, 12u, 228u, 52u, 212u,
    129u, 65u, 187u, 123u, 143u, 79u, 183u, 119u, 130u, 66u, 184u, 120u, 140u, 76u, 180u, 116u,
    33u, 193u, 17u, 251u, 47u, 207u, 31u, 247u, 34u, 194u, 18u, 248u, 44u, 204u, 28u, 244u,
    161u, 97u, 145u, 81u, 175u, 111u, 159u, 95u, 162u, 98u, 146u, 82u, 172u, 108u, 156u, 92u,
    9u, 225u, 49u, 209u, 5u, 239u, 63u, 223u, 10u, 226u, 50u, 210u, 6u, 236u, 60u, 220u,
    137u, 73u, 177u, 113u, 133u, 69u, 191u, 127u, 138u, 74u, 178u, 114u, 134u, 70u, 188u, 124u,
    41u, 201u, 25u, 241u, 37u, 197u, 21u, 255u, 42u, 202u, 26u, 242u, 38u, 198u, 22u, 252u,
    169u, 105u, 153u, 89u, 165u, 101u, 149u, 85u, 170u, 106u, 154u, 90u, 166u, 102u, 150u, 86u,
    3u, 233u, 57u, 217u, 13u, 229u, 53u, 213u, 0u, 234u, 58u, 218u, 14u, 230u, 54u, 214u,
    131u, 67u, 185u, 121u, 141u, 77u, 181u, 117u, 128u, 64u, 186u, 122u, 142u, 78u, 182u, 118u,
    35u, 195u, 19u, 249u, 45u, 205u, 29u, 245u, 32u, 192u, 16u, 250u, 46u, 206u, 30u, 246u,
    163u, 99u, 147u, 83u, 173u, 109u, 157u, 93u, 160u, 96u, 144u, 80u, 174u, 110u, 158u, 94u,
    11u, 227u, 51u, 211u, 7u, 237u, 61u, 221u, 8u, 224u, 48u, 208u, 4u, 238u, 62u, 222u,
    139u, 75u, 179u, 115u, 135u, 71u, 189u, 125u, 136u, 72u, 176u, 112u, 132u, 68u, 190u, 126u,
    43u, 203u, 27u, 243u, 39u, 199u, 23u, 253u, 40u, 200u, 24u, 240u, 36u, 196u, 20u, 254u,
    171u, 107u, 155u, 91u, 167u, 103u, 151u, 87u, 168u, 104u, 152u, 88u, 164u, 100u, 148u, 84u
);

fn rem_euclid_i32(a: i32, n: i32) -> i32 {
    let r = a % n;
    if (r < 0) {
        return r + n;
    }
    return r;
}

// Metal f32 rotation can sit 1 ULP above Rust; bias floor to match CPU `dither_ordered`.
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

fn bayer16_i32(gx: i32, gy: i32) -> f32 {
    let mx = rem_euclid_i32(gx, 16);
    let my = rem_euclid_i32(gy, 16);
    return f32(BAYER16[my * 16 + mx]) / 256.0;
}

fn clustered8_i32(gx: i32, gy: i32) -> f32 {
    let mx = rem_euclid_i32(gx, 8);
    let my = rem_euclid_i32(gy, 8);
    return f32(CLUSTERED8[my * 8 + mx]) / 64.0;
}

fn dispersed16_i32(gx: i32, gy: i32) -> f32 {
    let mx = rem_euclid_i32(gx, 16);
    let my = rem_euclid_i32(gy, 16);
    return f32(DISPERSED16[my * 16 + mx]) / 256.0;
}

fn sample_bayer(gx: i32, gy: i32, matrix: u32) -> f32 {
    let rot = rotate_with_trig(gx, gy, pat.packed.x, pat.packed.y);
    var t = 0.0;
    switch (matrix) {
        case 2u: { t = bayer2_i32(rot.x, rot.y); }
        case 4u: { t = bayer4_i32(rot.x, rot.y); }
        case 16u: { t = bayer16_i32(rot.x, rot.y); }
        case 108u: { t = clustered8_i32(rot.x, rot.y); }
        case 116u: { t = dispersed16_i32(rot.x, rot.y); }
        default: { t = bayer8_i32(rot.x, rot.y); }
    }
    return apply_threshold_bias(t, pat.packed.w);
}

fn round_away_from_zero(x: f32) -> f32 {
    if (x < 0.0) {
        return ceil(x - 0.5);
    }
    return floor(x + 0.5);
}

fn quantize_uniform(value: f32, levels: f32, offset: f32) -> f32 {
    let scaled = value * (levels - 1.0) + offset;
    return clamp(round_away_from_zero(scaled), 0.0, levels - 1.0) / (levels - 1.0);
}

fn luminance(r: f32, g: f32, b: f32) -> f32 {
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

fn dither_at(gid: vec3<u32>, threshold: f32) {
    if (gid.x >= CORE || gid.y >= CORE) {
        return;
    }
    let px = i32(gid.x + HALO);
    let py = i32(gid.y + HALO);
    let rgba = textureLoad(tile_in, vec2<i32>(px, py), 0);
    let levels = u.params.x;
    let threshold_scale = u.params.y;
    let color_mode = pat.packed.z;
    let dither_a = color_mode >= 1.5;
    let is_gray = (color_mode % 2.0) >= 0.5;
    let offset = (threshold - 0.5) * threshold_scale;

    var out = rgba;
    if (!is_gray) {
        out.r = quantize_uniform(rgba.r, levels, offset);
        out.g = quantize_uniform(rgba.g, levels, offset);
        out.b = quantize_uniform(rgba.b, levels, offset);
    } else {
        let lum = luminance(rgba.r, rgba.g, rgba.b);
        let q = quantize_uniform(lum, levels, offset);
        out = vec4<f32>(q, q, q, rgba.a);
    }
    if (dither_a) {
        if (rgba.a <= 0.0) {
            out.a = 0.0;
        } else if (rgba.a >= 1.0) {
            out.a = 1.0;
        } else if (rgba.a > threshold) {
            out.a = 1.0;
        } else {
            out.a = 0.0;
        }
    }
    if (dither_a && out.a <= 0.0) {
        out = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    textureStore(tile_out, vec2<i32>(px, py), out);
}

@compute @workgroup_size(16, 16)
fn bayer2_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    dither_at(gid, sample_bayer(gx, gy, 2u));
}

@compute @workgroup_size(16, 16)
fn bayer4_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    dither_at(gid, sample_bayer(gx, gy, 4u));
}

@compute @workgroup_size(16, 16)
fn bayer8_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    dither_at(gid, sample_bayer(gx, gy, 8u));
}

@compute @workgroup_size(16, 16)
fn bayer16_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    dither_at(gid, sample_bayer(gx, gy, 16u));
}

@compute @workgroup_size(16, 16)
fn clustered8_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    dither_at(gid, sample_bayer(gx, gy, 108u));
}

@compute @workgroup_size(16, 16)
fn dispersed16_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    dither_at(gid, sample_bayer(gx, gy, 116u));
}
