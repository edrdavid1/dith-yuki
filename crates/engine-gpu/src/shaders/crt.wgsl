// CRT scanline + RGB triad mask — ports CPU filters/crt.rs with global Y/X.

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct CrtUniforms {
    tile: TileUniforms,
    // period, strength, mask_strength, _
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: CrtUniforms;
@group(0) @binding(1) var<storage, read> input_px: array<f32>;
@group(0) @binding(2) var<storage, read_write> output_px: array<f32>;

fn rem_euclid_i(a: i32, b: i32) -> i32 {
    let r = a % b;
    if (r < 0) {
        return r + b;
    }
    return r;
}

fn scanline_gain(y_g: i32, period: i32, strength: f32) -> f32 {
    let line = rem_euclid_i(y_g, period);
    var dark_rows = period / 2;
    if (dark_rows < 1) {
        dark_rows = 1;
    }
    if (line < dark_rows) {
        return 1.0 - strength;
    }
    return 1.0;
}

fn rgb_mask_gain(x_g: i32, channel: u32, mask_strength: f32) -> f32 {
    if (mask_strength <= 0.0) {
        return 1.0;
    }
    let col = u32(rem_euclid_i(x_g, 3));
    if (col == channel) {
        return 1.0;
    }
    return 1.0 - mask_strength;
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= u.tile.size.x || gid.y >= u.tile.size.y) {
        return;
    }
    let idx = (gid.y * u.tile.size.x + gid.x) * 4u;
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    let period = i32(u.params.x);
    let strength = u.params.y;
    let mask_strength = u.params.z;
    let gain = scanline_gain(gy, period, strength);

    for (var c = 0u; c < 3u; c++) {
        let mask = rgb_mask_gain(gx, c, mask_strength);
        let v = input_px[idx + c] * gain * mask;
        output_px[idx + c] = clamp(v, 0.0, 1.0);
    }
    output_px[idx + 3u] = input_px[idx + 3u];
}
