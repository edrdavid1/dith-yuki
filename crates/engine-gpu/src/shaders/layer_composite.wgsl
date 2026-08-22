// Layer composite — Porter-Duff over + blend modes (Path B T7.5).
// Fused stack: one dispatch blends N resident array layers onto transparent,
// writing the Composite slot directly (no scratch ping-pong).

const HALO: u32 = 2u;
const CORE: u32 = 256u;
const MAX_STACK: u32 = 16u;

struct CompositeHeader {
    // x = layer_count, yzw unused — primary uniform ≤32 B on Metal
    params: vec4<f32>,
}

// Per layer: x=array layer index, y=blend_mode, z=opacity, w unused
struct LayerOp {
    packed: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: CompositeHeader;
@group(0) @binding(1) var<storage, read> ops: array<LayerOp>;
@group(0) @binding(2) var resident: texture_2d_array<f32>;
@group(0) @binding(3) var tile_out: texture_storage_2d<rgba32float, write>;

fn apply_blend_mode(mode: u32, s: f32, d: f32) -> f32 {
    switch (mode) {
        case 1u: { // Multiply
            return s * d;
        }
        case 2u: { // Screen
            return s + d - s * d;
        }
        case 3u: { // Overlay
            if (d < 0.5) {
                return 2.0 * s * d;
            }
            return 1.0 - 2.0 * (1.0 - s) * (1.0 - d);
        }
        case 4u: { // Darken
            return min(s, d);
        }
        case 5u: { // Lighten
            return max(s, d);
        }
        case 6u: { // ColorDodge
            if (s >= 1.0) {
                return 1.0;
            }
            return min(d / (1.0 - s), 1.0);
        }
        case 7u: { // ColorBurn
            if (s <= 0.0) {
                return 0.0;
            }
            return 1.0 - min((1.0 - d) / s, 1.0);
        }
        case 8u: { // HardLight
            if (s < 0.5) {
                return 2.0 * s * d;
            }
            return 1.0 - 2.0 * (1.0 - s) * (1.0 - d);
        }
        case 9u: { // SoftLight
            var dd: f32;
            if (d <= 0.25) {
                dd = ((16.0 * d - 12.0) * d + 4.0) * d;
            } else {
                dd = sqrt(d);
            }
            if (s <= 0.5) {
                return d - (1.0 - 2.0 * s) * d * (1.0 - d);
            }
            return d + (2.0 * s - 1.0) * (dd - d);
        }
        case 10u: { // Difference
            return abs(s - d);
        }
        case 11u: { // Exclusion
            return s + d - 2.0 * s * d;
        }
        default: { // Normal + reserved
            return s;
        }
    }
}

fn blend_over(dst: vec4<f32>, src_px: vec4<f32>, mode: u32, opacity: f32) -> vec4<f32> {
    let src_a = src_px.a * opacity;
    if (src_a < 1e-6) {
        return dst;
    }
    let dst_a = dst.a;
    let blended = vec3<f32>(
        apply_blend_mode(mode, src_px.r, dst.r),
        apply_blend_mode(mode, src_px.g, dst.g),
        apply_blend_mode(mode, src_px.b, dst.b),
    );
    let out_rgb = blended * src_a + dst.rgb * dst_a * (1.0 - src_a);
    let out_a = src_a + dst_a * (1.0 - src_a);
    return vec4<f32>(out_rgb, out_a);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= CORE || gid.y >= CORE) {
        return;
    }
    let px = i32(gid.x + HALO);
    let py = i32(gid.y + HALO);
    let count = min(u32(u.params.x), MAX_STACK);
    var acc = vec4<f32>(0.0);
    for (var i = 0u; i < count; i = i + 1u) {
        let op = ops[i].packed;
        let layer = u32(op.x);
        let mode = u32(op.y);
        let opacity = op.z;
        let src_px = textureLoad(resident, vec2<i32>(px, py), layer, 0);
        acc = blend_over(acc, src_px, mode, opacity);
    }
    textureStore(tile_out, vec2<i32>(px, py), acc);
}
