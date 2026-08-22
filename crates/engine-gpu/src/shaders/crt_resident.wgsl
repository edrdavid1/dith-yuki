// CRT scanline + RGB triad mask — resident storage textures (Path B).
// Ports CPU filters/crt.rs and v1 crt.wgsl with global Y/X.

const HALO: u32 = 2u;
const CORE: u32 = 256u;

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct CrtUniforms {
    tile: TileUniforms,
    // x=period, y=strength, z=mask_strength
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: CrtUniforms;
@group(0) @binding(1) var tile_in: texture_2d<f32>;
@group(0) @binding(2) var tile_out: texture_storage_2d<rgba32float, write>;

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
    if (gid.x >= CORE || gid.y >= CORE) {
        return;
    }
    let px = i32(gid.x + HALO);
    let py = i32(gid.y + HALO);
    let rgba = textureLoad(tile_in, vec2<i32>(px, py), 0);

    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);
    let period = i32(u.params.x);
    let strength = u.params.y;
    let mask_strength = u.params.z;
    let gain = scanline_gain(gy, period, strength);

    var out = rgba;
    out.r = clamp(rgba.r * gain * rgb_mask_gain(gx, 0u, mask_strength), 0.0, 1.0);
    out.g = clamp(rgba.g * gain * rgb_mask_gain(gx, 1u, mask_strength), 0.0, 1.0);
    out.b = clamp(rgba.b * gain * rgb_mask_gain(gx, 2u, mask_strength), 0.0, 1.0);
    // alpha preserved
    textureStore(tile_out, vec2<i32>(px, py), out);
}
