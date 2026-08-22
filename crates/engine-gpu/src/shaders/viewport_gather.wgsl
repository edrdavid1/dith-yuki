// Gather core 256×256 from a resident tile layer → RGBA8 storage buffer (one readback/tile).

const HALO: u32 = 2u;
const CORE: u32 = 256u;

@group(0) @binding(0) var tile_in: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> rgba8_out: array<u32>;

fn float_to_u8(v: f32) -> u32 {
    return u32(clamp(v, 0.0, 1.0) * 255.0 + 0.5);
}

@compute @workgroup_size(16, 16)
fn gather_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= CORE || gid.y >= CORE) {
        return;
    }
    let px = i32(gid.x + HALO);
    let py = i32(gid.y + HALO);
    let rgba = textureLoad(tile_in, vec2<i32>(px, py), 0);
    let idx = gid.y * CORE + gid.x;
    let r = float_to_u8(rgba.r);
    let g = float_to_u8(rgba.g);
    let b = float_to_u8(rgba.b);
    let a = float_to_u8(rgba.a);
    rgba8_out[idx] = r | (g << 8u) | (b << 16u) | (a << 24u);
}
