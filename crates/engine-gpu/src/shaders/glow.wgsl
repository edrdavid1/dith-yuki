// Soft glow / bloom — ports CPU filters/glow.rs (in-tile clamp path).
// Full tile including halo (260²) as RGBA32 storage buffers.

const TILE: u32 = 260u;
const KNEE: f32 = 0.05;

struct GlowUniforms {
    // x=radius, y=intensity, z=threshold, w=softness
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: GlowUniforms;
@group(0) @binding(1) var<storage, read> input_px: array<f32>;
@group(0) @binding(2) var<storage, read_write> output_px: array<f32>;
@group(0) @binding(3) var<storage, read_write> scratch_a: array<f32>;
@group(0) @binding(4) var<storage, read_write> scratch_b: array<f32>;

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

fn rgba_idx(x: u32, y: u32) -> u32 {
    return (y * TILE + x) * 4u;
}

fn rgb_idx(x: u32, y: u32) -> u32 {
    return (y * TILE + x) * 3u;
}

fn load_rgb(x: i32, y: i32) -> vec3<f32> {
    let sx = u32(clamp(x, 0, i32(TILE) - 1));
    let sy = u32(clamp(y, 0, i32(TILE) - 1));
    let i = rgba_idx(sx, sy);
    return vec3(input_px[i], input_px[i + 1u], input_px[i + 2u]);
}

fn sample_a(x: i32, y: i32) -> vec3<f32> {
    let sx = u32(clamp(x, 0, i32(TILE) - 1));
    let sy = u32(clamp(y, 0, i32(TILE) - 1));
    let i = rgb_idx(sx, sy);
    return vec3(scratch_a[i], scratch_a[i + 1u], scratch_a[i + 2u]);
}

fn sample_b(x: i32, y: i32) -> vec3<f32> {
    let sx = u32(clamp(x, 0, i32(TILE) - 1));
    let sy = u32(clamp(y, 0, i32(TILE) - 1));
    let i = rgb_idx(sx, sy);
    return vec3(scratch_b[i], scratch_b[i + 1u], scratch_b[i + 2u]);
}

fn store_a(x: u32, y: u32, v: vec3<f32>) {
    let i = rgb_idx(x, y);
    scratch_a[i] = v.r;
    scratch_a[i + 1u] = v.g;
    scratch_a[i + 2u] = v.b;
}

fn store_b(x: u32, y: u32, v: vec3<f32>) {
    let i = rgb_idx(x, y);
    scratch_b[i] = v.r;
    scratch_b[i + 1u] = v.g;
    scratch_b[i + 2u] = v.b;
}

@compute @workgroup_size(16, 16)
fn mask_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let rgb = load_rgb(i32(gid.x), i32(gid.y));
    let w = soft_weight(lum(rgb), u.params.z);
    store_a(gid.x, gid.y, rgb * w);
}

@compute @workgroup_size(16, 16)
fn blur_h_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let radius = i32(round(u.params.x));
    let diam = f32(2 * radius + 1);
    var acc = vec3(0.0);
    let x = i32(gid.x);
    let y = i32(gid.y);
    for (var dx = -radius; dx <= radius; dx++) {
        acc += sample_a(x + dx, y);
    }
    store_b(gid.x, gid.y, acc / diam);
}

@compute @workgroup_size(16, 16)
fn blur_v_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let radius = i32(round(u.params.x));
    let diam = f32(2 * radius + 1);
    var acc = vec3(0.0);
    let x = i32(gid.x);
    let y = i32(gid.y);
    for (var dy = -radius; dy <= radius; dy++) {
        acc += sample_b(x, y + dy);
    }
    store_a(gid.x, gid.y, acc / diam);
}

@compute @workgroup_size(16, 16)
fn composite_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let i = rgba_idx(gid.x, gid.y);
    let src = vec3(input_px[i], input_px[i + 1u], input_px[i + 2u]);
    let glow = sample_a(i32(gid.x), i32(gid.y)) * u.params.y;
    let out = clamp(src + glow, vec3(0.0), vec3(1.0));
    output_px[i] = out.r;
    output_px[i + 1u] = out.g;
    output_px[i + 2u] = out.b;
    output_px[i + 3u] = input_px[i + 3u];
}
