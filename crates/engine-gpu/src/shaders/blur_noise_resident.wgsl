// Blur + grain — resident storage textures (Path B).
// Ports CPU filters/blur_noise.rs in-tile clamp path (premultiplied box blur + hash noise).

const TILE: u32 = 260u;
const NOISE_CELL: u32 = 4u;

struct BlurNoiseUniforms {
    // xy = tile_offset in core pixels (coord.x * 256, coord.y * 256)
    tile_offset: vec2<u32>,
    _pad: vec2<u32>,
    // x = blur radius px (0 = skip blur), y = noise amount, zw unused
    params: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: BlurNoiseUniforms;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var in_tex: texture_2d<f32>;
@group(0) @binding(3) var out_tex: texture_storage_2d<rgba32float, write>;

fn load_clamped(tex: texture_2d<f32>, x: i32, y: i32) -> vec4<f32> {
    let sx = clamp(x, 0, i32(TILE) - 1);
    let sy = clamp(y, 0, i32(TILE) - 1);
    return textureLoad(tex, vec2<i32>(sx, sy), 0);
}

fn hash01(x: u32, y: u32, ch: u32) -> f32 {
    var n = x * 374761393u + y * 668265263u + ch * 1274126177u;
    n = (n ^ (n >> 13u)) * 1274126177u;
    n = n ^ (n >> 16u);
    return f32(n) / 4294967295.0;
}

// Premultiply RGB by alpha: src → out
@compute @workgroup_size(16, 16)
fn premultiply_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let p = vec2<i32>(i32(gid.x), i32(gid.y));
    let rgba = textureLoad(src_tex, p, 0);
    textureStore(out_tex, p, vec4(rgba.rgb * rgba.a, rgba.a));
}

// Horizontal box blur (premultiplied): in → out
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

// Vertical box blur (premultiplied): in → out
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

// Unpremultiply + optional noise: in (premul or straight if no blur) → out
// When radius==0, in_tex is unused and we read src_tex as straight RGB.
@compute @workgroup_size(16, 16)
fn finish_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= TILE || gid.y >= TILE) { return; }
    let p = vec2<i32>(i32(gid.x), i32(gid.y));
    let radius = i32(round(u.params.x));
    let noise_amt = u.params.y;
    var rgb: vec3<f32>;
    var a: f32;
    if (radius > 0) {
        let premul = textureLoad(in_tex, p, 0);
        a = premul.a;
        if (a <= 1e-6) {
            rgb = vec3(0.0);
        } else {
            rgb = clamp(premul.rgb / a, vec3(0.0), vec3(1.0));
        }
    } else {
        let src = textureLoad(src_tex, p, 0);
        rgb = src.rgb;
        a = src.a;
    }
    if (noise_amt > 1e-6) {
        let gx = u.tile_offset.x + gid.x;
        let gy = u.tile_offset.y + gid.y;
        let cx = gx / NOISE_CELL;
        let cy = gy / NOISE_CELL;
        let n = hash01(cx, cy, 0u) * 2.0 - 1.0;
        rgb = clamp(rgb + vec3(n * noise_amt), vec3(0.0), vec3(1.0));
    }
    textureStore(out_tex, p, vec4(rgb, a));
}
