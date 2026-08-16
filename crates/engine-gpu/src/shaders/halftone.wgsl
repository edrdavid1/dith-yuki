// CMYK angled-screen halftone — ports CPU dither_ordered::apply_cmyk_halftone (ps=1, no palette).

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct HalftoneUniforms {
    tile: TileUniforms,
    // cell_size, threshold_scale, _, _
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: HalftoneUniforms;
@group(0) @binding(1) var<storage, read> input_px: array<f32>;
@group(0) @binding(2) var<storage, read_write> output_px: array<f32>;

const PI: f32 = 3.141592653589793;
const DEG15: f32 = 15.0 * PI / 180.0;
const DEG75: f32 = 75.0 * PI / 180.0;
const DEG0: f32 = 0.0;
const DEG45: f32 = 45.0 * PI / 180.0;

fn rem_euclid_f(a: f32, b: f32) -> f32 {
    let r = a % b;
    if (r < 0.0) {
        return r + b;
    }
    return r;
}

fn rotated_cell_dist(gx: f32, gy: f32, s: f32, theta: f32) -> f32 {
    let cos_t = cos(theta);
    let sin_t = sin(theta);
    let xr = gx * cos_t + gy * sin_t;
    let yr = -gx * sin_t + gy * cos_t;
    let cx = rem_euclid_f(xr, s) - s * 0.5;
    let cy = rem_euclid_f(yr, s) - s * 0.5;
    return sqrt(cx * cx + cy * cy);
}

fn rgb_to_cmyk(r: f32, g: f32, b: f32) -> vec4<f32> {
    let k = 1.0 - max(r, max(g, b));
    if (k >= 1.0 - 1e-6) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let c = (1.0 - r - k) / (1.0 - k);
    let m = (1.0 - g - k) / (1.0 - k);
    let y = (1.0 - b - k) / (1.0 - k);
    return vec4<f32>(
        clamp(c, 0.0, 1.0),
        clamp(m, 0.0, 1.0),
        clamp(y, 0.0, 1.0),
        clamp(k, 0.0, 1.0)
    );
}

fn cmyk_to_rgb(c: f32, m: f32, y: f32, k: f32) -> vec3<f32> {
    return vec3<f32>(
        1.0 - min(1.0, c + k),
        1.0 - min(1.0, m + k),
        1.0 - min(1.0, y + k)
    );
}

fn channel_ink(dist: f32, tone: f32, s: f32, threshold_scale: f32) -> f32 {
    let r_max = (s * 0.5) * sqrt(tone) * threshold_scale;
    if (dist <= r_max) {
        return 1.0;
    }
    return 0.0;
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= u.tile.size.x || gid.y >= u.tile.size.y) {
        return;
    }
    let idx = (gid.y * u.tile.size.x + gid.x) * 4u;
    let gx = f32(u.tile.tile_offset.x + gid.x);
    let gy = f32(u.tile.tile_offset.y + gid.y);
    let s = u.params.x;
    let threshold_scale = u.params.y;

    let r = input_px[idx];
    let g = input_px[idx + 1u];
    let b = input_px[idx + 2u];
    let a = input_px[idx + 3u];

    let cmyk = rgb_to_cmyk(r, g, b);
    let angles = array<f32, 4>(DEG15, DEG75, DEG0, DEG45);
    var dots = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    for (var i = 0u; i < 4u; i++) {
        let dist = rotated_cell_dist(gx, gy, s, angles[i]);
        dots[i] = channel_ink(dist, cmyk[i], s, threshold_scale);
    }
    let rgb = cmyk_to_rgb(dots[0], dots[1], dots[2], dots[3]);
    output_px[idx] = rgb.r;
    output_px[idx + 1u] = rgb.g;
    output_px[idx + 2u] = rgb.b;
    let dither_a = u.params.z > 0.5;
    if (dither_a) {
        if (a <= 0.0) {
            output_px[idx + 3u] = 0.0;
        } else if (a >= 1.0) {
            output_px[idx + 3u] = 1.0;
        } else if (a > 0.5) {
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
