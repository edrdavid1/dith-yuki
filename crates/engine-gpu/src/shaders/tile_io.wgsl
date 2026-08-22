// Shared tile coordinate uniforms for GPU-resident passes (Path B).
// Ported from v1 `bayer.wgsl` / `dispatch::TileUniforms`.
//
// Note: wgpu loads WGSL as single modules; duplicate these structs in pass
// shaders until a build-time concat step exists.

const HALO: u32 = 2u;
const CORE: u32 = 256u;

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

// Global document pixel coordinate for a core invocation.
fn global_coord(tile: TileUniforms, local: vec2<u32>) -> vec2<u32> {
    return tile.tile_offset + local;
}

// Sample resident tile including halo border.
fn sample_halo(tile_in: texture_2d<f32>, local: vec2<u32>) -> vec4<f32> {
    let px = i32(local.x + HALO);
    let py = i32(local.y + HALO);
    return textureLoad(tile_in, vec2<i32>(px, py), 0);
}
