// Pointwise grade ops — resident storage textures (Path B).
// Ports CPU invert / threshold / posterize / adjust pointwise legs.

const HALO: u32 = 2u;
const CORE: u32 = 256u;

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct PointwiseUniforms {
    tile: TileUniforms,
    // x = op (0 invert, 1 threshold, 2 posterize, 3 adjust, 4 levels, 5 curves)
    // y = levels channel_mask / curves channel
    ctrl: vec4<u32>,
    // invert: unused
    // threshold: x = threshold
    // posterize: x = levels
    // adjust: x=exposure_gain, y=contrast_m, z=brightness, w=sat_m
    // levels: x=input_black, y=input_white, z=gamma, w=output_black
    p0: vec4<f32>,
    // adjust: x=shadows, y=highlights, z=temperature, w=tint
    // levels: x=output_white
    p1: vec4<f32>,
    // adjust: x=vibrance, y=hue_degrees, z/w unused
    p2: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: PointwiseUniforms;
@group(0) @binding(1) var tile_in: texture_2d<f32>;
@group(0) @binding(2) var tile_out: texture_storage_2d<rgba32float, write>;
@group(0) @binding(3) var<storage, read> lut1d: array<f32>;

fn luma709(rgb: vec3<f32>) -> f32 {
    return 0.2126 * rgb.r + 0.7152 * rgb.g + 0.0722 * rgb.b;
}

fn smoothstep01(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn quantize_channel(v: f32, levels: f32) -> f32 {
    let n = max(levels, 2.0);
    let steps = n - 1.0;
    return round(clamp(v, 0.0, 1.0) * steps) / steps;
}

fn apply_invert(rgba: vec4<f32>) -> vec4<f32> {
    return vec4(1.0 - rgba.rgb, rgba.a);
}

fn apply_threshold(rgba: vec4<f32>, threshold: f32) -> vec4<f32> {
    let v = select(0.0, 1.0, luma709(rgba.rgb) >= threshold);
    return vec4(v, v, v, rgba.a);
}

fn apply_posterize(rgba: vec4<f32>, levels: f32) -> vec4<f32> {
    return vec4(
        quantize_channel(rgba.r, levels),
        quantize_channel(rgba.g, levels),
        quantize_channel(rgba.b, levels),
        rgba.a,
    );
}

fn hue_to_rgb(p: f32, q: f32, t_in: f32) -> f32 {
    var t = t_in;
    if (t < 0.0) { t += 1.0; }
    if (t > 1.0) { t -= 1.0; }
    if (t < 1.0 / 6.0) {
        return p + (q - p) * 6.0 * t;
    }
    if (t < 0.5) {
        return q;
    }
    if (t < 2.0 / 3.0) {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    return p;
}

fn rgb_to_hsl(rgb: vec3<f32>) -> vec3<f32> {
    let maxc = max(rgb.r, max(rgb.g, rgb.b));
    let minc = min(rgb.r, min(rgb.g, rgb.b));
    let l = (maxc + minc) * 0.5;
    if (abs(maxc - minc) < 1e-8) {
        return vec3(0.0, 0.0, l);
    }
    let d = maxc - minc;
    let s = select(d / (maxc + minc), d / (2.0 - maxc - minc), l > 0.5);
    var h: f32;
    if (abs(maxc - rgb.r) < 1e-8) {
        var v = (rgb.g - rgb.b) / d;
        if (rgb.g < rgb.b) {
            v += 6.0;
        }
        h = v;
    } else if (abs(maxc - rgb.g) < 1e-8) {
        h = (rgb.b - rgb.r) / d + 2.0;
    } else {
        h = (rgb.r - rgb.g) / d + 4.0;
    }
    h /= 6.0;
    return vec3(h, s, l);
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> vec3<f32> {
    if (abs(s) < 1e-8) {
        return vec3(l, l, l);
    }
    let q = select(l + s - l * s, l * (1.0 + s), l < 0.5);
    let p = 2.0 * l - q;
    return vec3(
        hue_to_rgb(p, q, h + 1.0 / 3.0),
        hue_to_rgb(p, q, h),
        hue_to_rgb(p, q, h - 1.0 / 3.0),
    );
}

fn lut1d_lookup(x: f32) -> f32 {
    let n = arrayLength(&lut1d);
    if (n == 0u) {
        return x;
    }
    let max_i = f32(n - 1u);
    let xf = clamp(x, 0.0, 1.0) * max_i;
    let i0 = u32(xf);
    let i1 = min(i0 + 1u, n - 1u);
    let frac = xf - f32(i0);
    return lut1d[i0] * (1.0 - frac) + lut1d[i1] * frac;
}

fn apply_levels_value(pixel: f32) -> f32 {
    let input_black = u.p0.x;
    let input_white = u.p0.y;
    let gamma = u.p0.z;
    let output_black = u.p0.w;
    let output_white = u.p1.x;
    let input_range = input_white - input_black;
    if (abs(input_range) < 0.001) {
        return output_black;
    }
    var remapped = clamp((pixel - input_black) / input_range, 0.0, 1.0);
    if (abs(gamma) >= 0.001) {
        remapped = pow(remapped, 1.0 / gamma);
    }
    return clamp(output_black + remapped * (output_white - output_black), 0.0, 1.0);
}

fn apply_levels(rgba: vec4<f32>) -> vec4<f32> {
    let mask = u.ctrl.y;
    var r = rgba.r;
    var g = rgba.g;
    var b = rgba.b;
    if ((mask & 1u) != 0u) {
        r = apply_levels_value(r);
    } else {
        r = 0.0;
    }
    if ((mask & 2u) != 0u) {
        g = apply_levels_value(g);
    } else {
        g = 0.0;
    }
    if ((mask & 4u) != 0u) {
        b = apply_levels_value(b);
    } else {
        b = 0.0;
    }
    return vec4(r, g, b, rgba.a);
}

fn apply_curves(rgba: vec4<f32>) -> vec4<f32> {
    let ch = u.ctrl.y;
    // 0=R 1=G 2=B 3=All 4=Luminance(→green, matches CPU)
    if (ch == 0u) {
        return vec4(lut1d_lookup(rgba.r), rgba.g, rgba.b, rgba.a);
    }
    if (ch == 1u || ch == 4u) {
        return vec4(rgba.r, lut1d_lookup(rgba.g), rgba.b, rgba.a);
    }
    if (ch == 2u) {
        return vec4(rgba.r, rgba.g, lut1d_lookup(rgba.b), rgba.a);
    }
    return vec4(
        lut1d_lookup(rgba.r),
        lut1d_lookup(rgba.g),
        lut1d_lookup(rgba.b),
        rgba.a,
    );
}

fn apply_adjust(rgba: vec4<f32>) -> vec4<f32> {
    var r = rgba.r;
    var g = rgba.g;
    var b = rgba.b;
    let a = rgba.a;

    let exposure_gain = u.p0.x;
    let contrast_m = u.p0.y;
    let brightness = u.p0.z;
    let sat_m = u.p0.w;
    let shadows = u.p1.x;
    let highlights = u.p1.y;
    let temperature = u.p1.z;
    let tint = u.p1.w;
    let vibrance = u.p2.x;
    let hue_deg = u.p2.y;

    if (abs(exposure_gain - 1.0) > 1e-8) {
        r = clamp(r * exposure_gain, 0.0, 1.0);
        g = clamp(g * exposure_gain, 0.0, 1.0);
        b = clamp(b * exposure_gain, 0.0, 1.0);
    }

    r = (r - 0.5) * contrast_m + 0.5 + brightness;
    g = (g - 0.5) * contrast_m + 0.5 + brightness;
    b = (b - 0.5) * contrast_m + 0.5 + brightness;
    let lum = luma709(vec3(r, g, b));
    r = lum + (r - lum) * sat_m;
    g = lum + (g - lum) * sat_m;
    b = lum + (b - lum) * sat_m;
    if (a <= 1e-6) {
        return vec4(0.0, 0.0, 0.0, a);
    }
    r = clamp(r, 0.0, 1.0);
    g = clamp(g, 0.0, 1.0);
    b = clamp(b, 0.0, 1.0);

    if (shadows != 0.0 || highlights != 0.0) {
        let y = luma709(vec3(r, g, b));
        let shadow_m = 1.0 - smoothstep01(0.0, 0.5, y);
        let highlight_m = smoothstep01(0.5, 1.0, y);
        let gain = max(1.0 + shadows * shadow_m + highlights * highlight_m, 0.0);
        r = clamp(r * gain, 0.0, 1.0);
        g = clamp(g * gain, 0.0, 1.0);
        b = clamp(b * gain, 0.0, 1.0);
    }

    if (temperature != 0.0 || tint != 0.0) {
        let strength = 0.25;
        r = clamp(r * (1.0 + strength * temperature - 0.5 * strength * tint), 0.0, 1.0);
        g = clamp(g * (1.0 + strength * tint), 0.0, 1.0);
        b = clamp(b * (1.0 - strength * temperature - 0.5 * strength * tint), 0.0, 1.0);
    }

    if (vibrance != 0.0) {
        let maxc = max(r, max(g, b));
        let minc = min(r, min(g, b));
        let sat = select(0.0, (maxc - minc) / maxc, maxc > 1e-6);
        let weight = select(sat, 1.0 - sat, vibrance >= 0.0);
        let scale = 1.0 + vibrance * weight;
        let y = luma709(vec3(r, g, b));
        r = clamp(y + (r - y) * scale, 0.0, 1.0);
        g = clamp(y + (g - y) * scale, 0.0, 1.0);
        b = clamp(y + (b - y) * scale, 0.0, 1.0);
    }

    if (hue_deg != 0.0) {
        let hsl = rgb_to_hsl(vec3(r, g, b));
        var nh = hsl.x + hue_deg / 360.0;
        nh = nh - floor(nh);
        let rgb = hsl_to_rgb(nh, hsl.y, hsl.z);
        r = clamp(rgb.r, 0.0, 1.0);
        g = clamp(rgb.g, 0.0, 1.0);
        b = clamp(rgb.b, 0.0, 1.0);
    }

    return vec4(r, g, b, a);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= CORE || gid.y >= CORE) {
        return;
    }
    let px = i32(gid.x + HALO);
    let py = i32(gid.y + HALO);
    let rgba = textureLoad(tile_in, vec2<i32>(px, py), 0);
    let op = u.ctrl.x;
    var out = rgba;
    if (op == 0u) {
        out = apply_invert(rgba);
    } else if (op == 1u) {
        out = apply_threshold(rgba, clamp(u.p0.x, 0.0, 1.0));
    } else if (op == 2u) {
        out = apply_posterize(rgba, clamp(u.p0.x, 2.0, 64.0));
    } else if (op == 3u) {
        out = apply_adjust(rgba);
    } else if (op == 4u) {
        out = apply_levels(rgba);
    } else if (op == 5u) {
        out = apply_curves(rgba);
    }
    textureStore(tile_out, vec2<i32>(px, py), out);
}
