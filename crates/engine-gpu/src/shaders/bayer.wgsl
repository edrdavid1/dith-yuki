// Bayer ordered dither — exact integer matrix thresholds matching CPU dither_ordered.
// Global coords: tile_offset + local. Workgroup 16×16.

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct BayerUniforms {
    tile: TileUniforms,
    // matrix_n (unused in entry; levels, threshold_scale, color_mode)
    // color_mode: 0=rgb, 1=gray, +2 if dither_alpha
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

fn bayer16(gx: u32, gy: u32) -> f32 {
    let mx = gx % 16u;
    let my = gy % 16u;
    return f32(BAYER16[my * 16u + mx]) / 256.0;
}

fn clustered8(gx: u32, gy: u32) -> f32 {
    let mx = gx % 8u;
    let my = gy % 8u;
    return f32(CLUSTERED8[my * 8u + mx]) / 64.0;
}

fn dispersed16(gx: u32, gy: u32) -> f32 {
    let mx = gx % 16u;
    let my = gy % 16u;
    return f32(DISPERSED16[my * 16u + mx]) / 256.0;
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
    let dither_a = color_mode >= 1.5;
    let is_gray = (color_mode % 2.0) >= 0.5;
    let offset = (threshold - 0.5) * threshold_scale;

    let r = input_px[idx];
    let g = input_px[idx + 1u];
    let b = input_px[idx + 2u];
    let a = input_px[idx + 3u];

    if (!is_gray) {
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
    if (dither_a) {
        if (a <= 0.0) {
            output_px[idx + 3u] = 0.0;
        } else if (a >= 1.0) {
            output_px[idx + 3u] = 1.0;
        } else if (a > threshold) {
            output_px[idx + 3u] = 1.0;
        } else {
            output_px[idx + 3u] = 0.0;
        }
    } else {
        output_px[idx + 3u] = a;
    }
    if (dither_a && output_px[idx + 3u] <= 0.0) {
        output_px[idx] = 0.0;
        output_px[idx + 1u] = 0.0;
        output_px[idx + 2u] = 0.0;
        output_px[idx + 3u] = 0.0;
    }
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

@compute @workgroup_size(16, 16)
fn bayer16_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = u.tile.tile_offset.x + gid.x;
    let gy = u.tile.tile_offset.y + gid.y;
    dither_at(gid, bayer16(gx, gy));
}

@compute @workgroup_size(16, 16)
fn clustered8_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = u.tile.tile_offset.x + gid.x;
    let gy = u.tile.tile_offset.y + gid.y;
    dither_at(gid, clustered8(gx, gy));
}

@compute @workgroup_size(16, 16)
fn dispersed16_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let gx = u.tile.tile_offset.x + gid.x;
    let gy = u.tile.tile_offset.y + gid.y;
    dither_at(gid, dispersed16(gx, gy));
}
