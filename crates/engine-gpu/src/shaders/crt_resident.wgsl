// CRT scanline + mask + light screen traits — resident textures (Path B).
// Ports CPU filters/crt.rs / crt.wgsl with global Y/X and HALO sampling.

const HALO: u32 = 2u;
const CORE: u32 = 256u;
const HALO_F: f32 = 2.0;
const TAU: f32 = 6.28318530718;

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct CrtUniforms {
    tile: TileUniforms,
    // period, strength, mask_strength, soft_scanlines
    p0: vec4<f32>,
    // line_width, orientation, mask_type, brightness_compensate
    p1: vec4<f32>,
    // curvature, vignette, phosphor_bleed, sharpness
    p2: vec4<f32>,
    // doc_w, doc_h, scale, _
    p3: vec4<f32>,
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

fn div_euclid_i(a: i32, b: i32) -> i32 {
    let q = a / b;
    let r = a % b;
    if (r != 0 && ((a < 0) != (b < 0))) {
        return q - 1;
    }
    return q;
}

fn crt_cell(coord: i32, scale: i32) -> i32 {
    let s = max(scale, 1);
    return div_euclid_i(coord, s);
}

fn scanline_gain(axis: i32, period: i32, strength: f32, line_width: f32, soft: f32) -> f32 {
    let p = max(period, 2);
    let line = rem_euclid_i(axis, p);
    let t = f32(line) / f32(p);
    var dark_rows = i32(round(line_width));
    dark_rows = clamp(dark_rows, 1, p);
    let frac = f32(dark_rows) / f32(p);
    let soft_a = clamp(soft, 0.0, 1.0);
    let s = clamp(strength, 0.0, 1.0);

    var hard = 1.0;
    if (line < dark_rows) {
        hard = 1.0 - s;
    }

    let cos01 = 0.5 + 0.5 * cos(t * TAU);
    let shaped = pow(cos01, 1.0 + (1.0 - frac) * 2.0);
    let soft_gain = 1.0 - s * shaped;
    return hard * (1.0 - soft_a) + soft_gain * soft_a;
}

fn rgb_mask_gain(x_g: i32, y_g: i32, channel: u32, mask_strength: f32, mask_type: f32) -> f32 {
    if (mask_strength <= 0.0) {
        return 1.0;
    }
    var col: i32;
    if (mask_type >= 0.5) {
        col = rem_euclid_i(x_g + y_g * 2, 3);
    } else {
        col = rem_euclid_i(x_g, 3);
    }
    if (u32(col) == channel) {
        return 1.0;
    }
    return 1.0 - mask_strength;
}

fn mean_scanline_gain(strength: f32, period: i32, line_width: f32, soft: f32) -> f32 {
    let s = clamp(strength, 0.0, 1.0);
    let soft_a = clamp(soft, 0.0, 1.0);
    let p = max(period, 2);
    var dark_rows = i32(round(line_width));
    dark_rows = clamp(dark_rows, 1, p);
    let frac = f32(dark_rows) / f32(p);
    let hard_mean = 1.0 - s * frac;
    let soft_mean = 1.0 - s * 0.5;
    return hard_mean * (1.0 - soft_a) + soft_mean * soft_a;
}

fn compensate_scale(strength: f32, period: i32, line_width: f32, soft: f32, mask_strength: f32, amount: f32) -> f32 {
    let c = clamp(amount, 0.0, 1.0);
    if (c <= 0.0) {
        return 1.0;
    }
    var mask_mean = 1.0;
    if (mask_strength > 0.0) {
        mask_mean = 1.0 - (2.0 / 3.0) * clamp(mask_strength, 0.0, 1.0);
    }
    let mean = max(mean_scanline_gain(strength, period, line_width, soft) * mask_mean, 1e-3);
    let full = 1.0 / mean;
    return 1.0 + (full - 1.0) * c;
}

fn doc_uv(gx: i32, gy: i32, doc_w: f32, doc_h: f32) -> vec2<f32> {
    let w = max(doc_w, 1.0);
    let h = max(doc_h, 1.0);
    return vec2<f32>(
        (f32(gx) + 0.5) / w * 2.0 - 1.0,
        (f32(gy) + 0.5) / h * 2.0 - 1.0,
    );
}

fn barrel_offset(uv: vec2<f32>, curvature: f32) -> vec2<f32> {
    let k = clamp(curvature, 0.0, 1.0);
    if (k <= 0.0) {
        return vec2<f32>(0.0, 0.0);
    }
    let r2 = dot(uv, uv);
    let factor = k * 0.35 * r2;
    let scale = f32(CORE) * 0.25;
    return clamp(uv * factor * scale, vec2<f32>(-HALO_F, -HALO_F), vec2<f32>(HALO_F, HALO_F));
}

fn vignette_gain(uv: vec2<f32>, amount: f32) -> f32 {
    let a = clamp(amount, 0.0, 1.0);
    if (a <= 0.0) {
        return 1.0;
    }
    let r = length(uv);
    let t = clamp((r - 0.55) / 0.85, 0.0, 1.0);
    return 1.0 - a * t * t;
}

fn load_rgba(lx: i32, ly: i32) -> vec4<f32> {
    let maxi = i32(CORE + 2u * HALO) - 1;
    let x = clamp(lx, 0, maxi);
    let y = clamp(ly, 0, maxi);
    return textureLoad(tile_in, vec2<i32>(x, y), 0);
}

fn sample_bilinear(fx: f32, fy: f32) -> vec4<f32> {
    let maxf = f32(CORE + 2u * HALO) - 1.0;
    let fx2 = clamp(fx, 0.0, maxf);
    let fy2 = clamp(fy, 0.0, maxf);
    let x0 = i32(floor(fx2));
    let y0 = i32(floor(fy2));
    let x1 = min(x0 + 1, i32(maxf));
    let y1 = min(y0 + 1, i32(maxf));
    let tx = fx2 - f32(x0);
    let ty = fy2 - f32(y0);
    let c00 = load_rgba(x0, y0);
    let c10 = load_rgba(x1, y0);
    let c01 = load_rgba(x0, y1);
    let c11 = load_rgba(x1, y1);
    let a = c00 * (1.0 - tx) + c10 * tx;
    let b = c01 * (1.0 - tx) + c11 * tx;
    return a * (1.0 - ty) + b * ty;
}

fn phosphor_bleed(center: vec4<f32>, left: vec4<f32>, right: vec4<f32>, amount: f32) -> vec4<f32> {
    let a = clamp(amount, 0.0, 1.0);
    if (a <= 0.0) {
        return center;
    }
    return vec4<f32>(
        center.r * (1.0 - 0.55 * a) + left.r * (0.55 * a),
        center.g * (1.0 - 0.25 * a) + left.g * (0.125 * a) + right.g * (0.125 * a),
        center.b * (1.0 - 0.55 * a) + right.b * (0.55 * a),
        center.a,
    );
}

fn sharpen(center: vec4<f32>, n: vec4<f32>, s: vec4<f32>, e: vec4<f32>, w: vec4<f32>, amount: f32) -> vec4<f32> {
    let a = clamp(amount, 0.0, 1.0);
    if (a <= 0.0) {
        return center;
    }
    let blur = (n + s + e + w) * 0.25;
    return vec4<f32>(
        clamp(center.r + (center.r - blur.r) * a, 0.0, 1.0),
        clamp(center.g + (center.g - blur.g) * a, 0.0, 1.0),
        clamp(center.b + (center.b - blur.b) * a, 0.0, 1.0),
        center.a,
    );
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= CORE || gid.y >= CORE) {
        return;
    }
    let px = i32(gid.x + HALO);
    let py = i32(gid.y + HALO);
    let gx = i32(u.tile.tile_offset.x + gid.x);
    let gy = i32(u.tile.tile_offset.y + gid.y);

    let period = i32(u.p0.x);
    let strength = u.p0.y;
    let mask_strength = u.p0.z;
    let soft = u.p0.w;
    let line_width = u.p1.x;
    let orientation = u.p1.y;
    let mask_type = u.p1.z;
    let bright_comp = u.p1.w;
    let curvature = u.p2.x;
    let vignette = u.p2.y;
    let bleed = u.p2.z;
    let sharp = u.p2.w;
    let doc_w = u.p3.x;
    let doc_h = u.p3.y;
    let scale = max(i32(round(u.p3.z)), 1);

    let uv = doc_uv(gx, gy, doc_w, doc_h);
    let cx = crt_cell(gx, scale);
    let cy = crt_cell(gy, scale);
    let need_spatial = curvature > 1e-6 || bleed > 1e-6 || sharp > 1e-6;
    let bleed_step = clamp(f32(scale), 1.0, HALO_F);

    var rgba: vec4<f32>;
    if (need_spatial) {
        let off = barrel_offset(uv, curvature);
        let fx = f32(px) + off.x;
        let fy = f32(py) + off.y;
        let center = sample_bilinear(fx, fy);
        var lx = fx - bleed_step;
        var ly = fy;
        var rx = fx + bleed_step;
        var ry = fy;
        if (orientation >= 0.5) {
            lx = fx;
            ly = fy - bleed_step;
            rx = fx;
            ry = fy + bleed_step;
        }
        let left = sample_bilinear(lx, ly);
        let right = sample_bilinear(rx, ry);
        let bled = phosphor_bleed(center, left, right, bleed);
        let n = sample_bilinear(fx, fy - 1.0);
        let s = sample_bilinear(fx, fy + 1.0);
        let e = sample_bilinear(fx + 1.0, fy);
        let w = sample_bilinear(fx - 1.0, fy);
        rgba = sharpen(bled, n, s, e, w, sharp);
    } else {
        rgba = load_rgba(px, py);
    }

    var axis = cy;
    if (orientation >= 0.5) {
        axis = cx;
    }
    let gain = scanline_gain(axis, period, strength, line_width, soft);
    let vig = vignette_gain(uv, vignette);
    let comp = compensate_scale(strength, period, line_width, soft, mask_strength, bright_comp);

    var out = rgba;
    out.r = clamp(rgba.r * gain * rgb_mask_gain(cx, cy, 0u, mask_strength, mask_type) * vig * comp, 0.0, 1.0);
    out.g = clamp(rgba.g * gain * rgb_mask_gain(cx, cy, 1u, mask_strength, mask_type) * vig * comp, 0.0, 1.0);
    out.b = clamp(rgba.b * gain * rgb_mask_gain(cx, cy, 2u, mask_strength, mask_type) * vig * comp, 0.0, 1.0);
    textureStore(tile_out, vec2<i32>(px, py), out);
}
