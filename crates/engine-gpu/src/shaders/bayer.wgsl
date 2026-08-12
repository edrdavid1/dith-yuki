// Bayer ordered dither — exact integer matrix thresholds matching CPU dither_ordered.
// Global coords: tile_offset + local. Workgroup 16×16.

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct BayerUniforms {
    tile: TileUniforms,
    // matrix_n (unused in entry; levels, threshold_scale, color_mode)
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: BayerUniforms;
@group(0) @binding(1) var<storage, read> input_px: array<f32>;
@group(0) @binding(2) var<storage, read_write> output_px: array<f32>;

// Module-scope const tables (function-local large arrays are unreliable on some backends).
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

fn bayer2(gx: u32, gy: u32) -> f32 {
    let mx = gx % 2u;
    let my = gy % 2u;
    return f32(BAYER2[my * 2u + mx]) / 4.0;
}

fn bayer4(gx: u32, gy: u32) -> f32 {
    let mx = gx % 4u;
    let my = gy % 4u;
    return f32(BAYER4[my * 4u + mx]) / 16.0;
}

fn bayer8(gx: u32, gy: u32) -> f32 {
    let mx = gx % 8u;
    let my = gy % 8u;
    return f32(BAYER8[my * 8u + mx]) / 64.0;
}

fn round_away_from_zero(x: f32) -> f32 {
    // Match Rust f32::round (half away from zero), not WGSL round (ties-to-even).
    if (x < 0.0) {
        return ceil(x - 0.5);
    }
    return floor(x + 0.5);
}

fn quantize_uniform(value: f32, levels: f32, offset: f32) -> f32 {
    let scaled = value * (levels - 1.0) + offset;
    let quantized = clamp(round_away_from_zero(scaled), 0.0, levels - 1.0) / (levels - 1.0);
    return quantized;
}

fn luminance(r: f32, g: f32, b: f32) -> f32 {
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

fn dither_at(gid: vec3<u32>, threshold: f32) {
    if (gid.x >= u.tile.size.x || gid.y >= u.tile.size.y) {
        return;
    }
    let idx = (gid.y * u.tile.size.x + gid.x) * 4u;
    let levels = u.params.y;
    let threshold_scale = u.params.z;
    let color_mode = u.params.w;
    let offset = (threshold - 0.5) * threshold_scale;

    let r = input_px[idx];
    let g = input_px[idx + 1u];
    let b = input_px[idx + 2u];
    let a = input_px[idx + 3u];

    if (color_mode < 0.5) {
        output_px[idx] = quantize_uniform(r, levels, offset);
        output_px[idx + 1u] = quantize_uniform(g, levels, offset);
        output_px[idx + 2u] = quantize_uniform(b, levels, offset);
    } else {
        let lum = luminance(r, g, b);
        let q = quantize_uniform(lum, levels, offset);
        output_px[idx] = q;
        output_px[idx + 1u] = q;
        output_px[idx + 2u] = q;
    }
    output_px[idx + 3u] = a;
}

@compute @workgroup_size(16, 16)
fn bayer2_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = u.tile.tile_offset.x + gid.x;
    let gy = u.tile.tile_offset.y + gid.y;
    dither_at(gid, bayer2(gx, gy));
}

@compute @workgroup_size(16, 16)
fn bayer4_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = u.tile.tile_offset.x + gid.x;
    let gy = u.tile.tile_offset.y + gid.y;
    dither_at(gid, bayer4(gx, gy));
}

@compute @workgroup_size(16, 16)
fn bayer8_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = u.tile.tile_offset.x + gid.x;
    let gy = u.tile.tile_offset.y + gid.y;
    dither_at(gid, bayer8(gx, gy));
}
