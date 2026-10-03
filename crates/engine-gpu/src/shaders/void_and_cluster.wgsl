// Void-and-cluster ordered dither — cold RGBA32 buffer path.
// Thresholds from a 64² rank buffer (binding 3); quantize matches Bayer/CPU.

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct VacUniforms {
    tile: TileUniforms,
    // levels, threshold_scale, color_mode, unused
    // color_mode: 0=rgb, 1=gray, +2 if dither_alpha
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: VacUniforms;
@group(0) @binding(1) var<storage, read> input_px: array<f32>;
@group(0) @binding(2) var<storage, read_write> output_px: array<f32>;
@group(0) @binding(3) var<storage, read> ranks: array<u32>;

const VAC_SIZE: u32 = 64u;
const VAC_N: f32 = 4096.0;

fn rem_euclid_u32(a: u32, n: u32) -> u32 {
    return a % n;
}

fn vac_threshold(gx: u32, gy: u32) -> f32 {
    let mx = rem_euclid_u32(gx, VAC_SIZE);
    let my = rem_euclid_u32(gy, VAC_SIZE);
    return f32(ranks[my * VAC_SIZE + mx]) / VAC_N;
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

@compute @workgroup_size(16, 16)
fn vac_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= u.tile.size.x || gid.y >= u.tile.size.y) {
        return;
    }
    let gx = u.tile.tile_offset.x + gid.x;
    let gy = u.tile.tile_offset.y + gid.y;
    let threshold = vac_threshold(gx, gy);
    let idx = (gid.y * u.tile.size.x + gid.x) * 4u;
    let levels = u.params.x;
    let threshold_scale = u.params.y;
    let color_mode = u.params.z;
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
