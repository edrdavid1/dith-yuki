// Soft glow / bloom — resident storage textures (Path B).
// Ports CPU filters/glow.rs in-tile clamp path. Full 260² including halo.

const TILE: u32 = 260u;
const KNEE: f32 = 0.05;

struct GlowUniforms {
    // x=radius, y=intensity, z=threshold, w=_
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: GlowUniforms;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var in_tex: texture_2d<f32>;
@group(0) @binding(3) var out_tex: texture_storage_2d<rgba32float, write>;

fn lum(rgb: vec3<f32>) -> f32 {
    return 0.2126 * rgb.r + 0.7152 * rgb.g + 0.0722 * rgb.b;
}

fn soft_weight(l: f32, threshold: f32) -> f32 {
    if (threshold <= 0.0) {
        return 1.0;
    }
    let lo = max(threshold - KNEE, 0.0);
    let hi = min(threshold + KNEE, 1.0);
    if (hi <= lo) {
        if (l >= threshold) { return 1.0; }
        return 0.0;
    }
    let t = clamp((l - lo) / (hi - lo), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn load_clamped(tex: texture_2d<f32>, x: i32, y: i32) -> vec4<f32> {
    let sx = clamp(x, 0, i32(TILE) - 1);
    let sy = clamp(y, 0, i32(TILE) - 1);
    return textureLoad(tex, vec2<i32>(sx, sy), 0);
}

// Build bright mask from src_tex → out_tex
@compute @workgroup_size(16, 16)
fn mask_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let rgba = textureLoad(src_tex, vec2<i32>(i32(gid.x), i32(gid.y)), 0);
    let w = soft_weight(lum(rgba.rgb), u.params.z);
    textureStore(out_tex, vec2<i32>(i32(gid.x), i32(gid.y)), vec4(rgba.rgb * w, rgba.a));
}

// Horizontal box blur: in_tex → out_tex
@compute @workgroup_size(16, 16)
fn blur_h_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let radius = i32(round(u.params.x));
    let diam = f32(2 * radius + 1);
    var acc = vec3(0.0);
    let x = i32(gid.x);
    let y = i32(gid.y);
    for (var dx = -radius; dx <= radius; dx++) {
        acc += load_clamped(in_tex, x + dx, y).rgb;
    }
    let a = load_clamped(in_tex, x, y).a;
    textureStore(out_tex, vec2<i32>(x, y), vec4(acc / diam, a));
}

// Vertical box blur: in_tex → out_tex
@compute @workgroup_size(16, 16)
fn blur_v_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let radius = i32(round(u.params.x));
    let diam = f32(2 * radius + 1);
    var acc = vec3(0.0);
    let x = i32(gid.x);
    let y = i32(gid.y);
    for (var dy = -radius; dy <= radius; dy++) {
        acc += load_clamped(in_tex, x, y + dy).rgb;
    }
    let a = load_clamped(in_tex, x, y).a;
    textureStore(out_tex, vec2<i32>(x, y), vec4(acc / diam, a));
}

// Composite: src_tex + intensity * in_tex → out_tex
@compute @workgroup_size(16, 16)
fn composite_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let p = vec2<i32>(i32(gid.x), i32(gid.y));
    let src = textureLoad(src_tex, p, 0);
    let glow = textureLoad(in_tex, p, 0).rgb * u.params.y;
    let rgb = clamp(src.rgb + glow, vec3(0.0), vec3(1.0));
    textureStore(out_tex, p, vec4(rgb, src.a));
}
