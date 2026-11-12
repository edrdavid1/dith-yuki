// Detail (clarity + unsharp) — resident storage textures (Path B).
// Ports CPU filters/detail.rs + clarity.rs in-tile clamp path.

const TILE: u32 = 260u;
const CLARITY_RADIUS: i32 = 12;
const SHARP_RADIUS: i32 = 2;
const AMOUNT_SCALE: f32 = 1.5;

struct DetailUniforms {
    // x = clarity amount [-1,1], y = sharpness amount [0,2], zw unused
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: DetailUniforms;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var in_tex: texture_2d<f32>;
@group(0) @binding(3) var out_tex: texture_storage_2d<rgba32float, write>;

fn lum(rgb: vec3<f32>) -> f32 {
    return 0.2126 * rgb.r + 0.7152 * rgb.g + 0.0722 * rgb.b;
}

fn midtone_weight(l: f32) -> f32 {
    return clamp(4.0 * l * (1.0 - l), 0.0, 1.0);
}

fn load_clamped(tex: texture_2d<f32>, x: i32, y: i32) -> vec4<f32> {
    let sx = clamp(x, 0, i32(TILE) - 1);
    let sy = clamp(y, 0, i32(TILE) - 1);
    return textureLoad(tex, vec2<i32>(sx, sy), 0);
}

// Copy src → out (identity / pass start)
@compute @workgroup_size(16, 16)
fn copy_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let p = vec2<i32>(i32(gid.x), i32(gid.y));
    textureStore(out_tex, p, textureLoad(src_tex, p, 0));
}

// Premultiply: src → out
@compute @workgroup_size(16, 16)
fn premultiply_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let p = vec2<i32>(i32(gid.x), i32(gid.y));
    let rgba = textureLoad(src_tex, p, 0);
    textureStore(out_tex, p, vec4(rgba.rgb * rgba.a, rgba.a));
}

@compute @workgroup_size(16, 16)
fn blur_h_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    // params.z encodes active radius for this blur pass
    let radius = i32(round(u.params.z));
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

@compute @workgroup_size(16, 16)
fn blur_v_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let radius = i32(round(u.params.z));
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

// Clarity residual: straight_src in src_tex, premul working in in_tex (blurred),
// premul original also needed — use binding: src_tex=straight original,
// in_tex=blurred premul, and we need premul original. Pass premul original as
//... actually encode copies: after blur_v, in=blurred, we still have premul in scratch.
// Encode path stores premul in scratch_a, blurred in scratch_b; clarity_main reads
// src=straight, in=blurred, and we pass premul via... simpler: clarity reads
// straight from src_tex and recomputes premul as straight*a for orig.
@compute @workgroup_size(16, 16)
fn clarity_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let p = vec2<i32>(i32(gid.x), i32(gid.y));
    let straight = textureLoad(src_tex, p, 0);
    let blurred = textureLoad(in_tex, p, 0);
    let a = straight.a;
    let orig = straight.rgb * a; // match CPU: premul orig
    let low = blurred.rgb;
    let w = midtone_weight(lum(straight.rgb));
    let gain = u.params.x * AMOUNT_SCALE;
    let rgb = clamp(orig + gain * w * (orig - low), vec3(0.0), vec3(1.0));
    // Still premul-ish values; unpremultiply next
    textureStore(out_tex, p, vec4(rgb, a));
}

@compute @workgroup_size(16, 16)
fn unpremultiply_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let p = vec2<i32>(i32(gid.x), i32(gid.y));
    let rgba = textureLoad(in_tex, p, 0);
    var rgb: vec3<f32>;
    if (rgba.a <= 1e-6) {
        rgb = vec3(0.0);
    } else {
        rgb = clamp(rgba.rgb / rgba.a, vec3(0.0), vec3(1.0));
    }
    textureStore(out_tex, p, vec4(rgb, rgba.a));
}

// Unsharp: src=straight working image, in=blurred premul of working
@compute @workgroup_size(16, 16)
fn sharpen_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let p = vec2<i32>(i32(gid.x), i32(gid.y));
    let straight = textureLoad(src_tex, p, 0);
    let blurred = textureLoad(in_tex, p, 0);
    let a = straight.a;
    let orig = straight.rgb * a;
    let low = blurred.rgb;
    let amount = u.params.y * AMOUNT_SCALE;
    let rgb_pre = clamp(orig + amount * (orig - low), vec3(0.0), vec3(1.0));
    var rgb: vec3<f32>;
    if (a <= 1e-6) {
        rgb = vec3(0.0);
    } else {
        rgb = clamp(rgb_pre / a, vec3(0.0), vec3(1.0));
    }
    textureStore(out_tex, p, vec4(rgb, a));
}
