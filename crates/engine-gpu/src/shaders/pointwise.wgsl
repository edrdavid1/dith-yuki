// Pointwise grade ops — v1 storage-buffer path (parity / cold GPU).

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct PointwiseUniforms {
    tile: TileUniforms,
    ctrl: vec4<u32>,
    p0: vec4<f32>,
    p1: vec4<f32>,
    p2: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: PointwiseUniforms;
@group(0) @binding(1) var<storage, read> input_px: array<f32>;
@group(0) @binding(2) var<storage, read_write> output_px: array<f32>;
@group(0) @binding(3) var<storage, read> lut1d: array<f32>;

fn luma709(r: f32, g: f32, b: f32) -> f32 {
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
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

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> vec3<f32> {
    let maxc = max(r, max(g, b));
    let minc = min(r, min(g, b));
    let l = (maxc + minc) * 0.5;
    if (abs(maxc - minc) < 1e-8) {
        return vec3(0.0, 0.0, l);
    }
    let d = maxc - minc;
    let s = select(d / (maxc + minc), d / (2.0 - maxc - minc), l > 0.5);
    var h: f32;
    if (abs(maxc - r) < 1e-8) {
        var v = (g - b) / d;
        if (g < b) {
            v += 6.0;
        }
        h = v;
    } else if (abs(maxc - g) < 1e-8) {
        h = (b - r) / d + 2.0;
    } else {
        h = (r - g) / d + 4.0;
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

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= u.tile.size.x || gid.y >= u.tile.size.y) {
        return;
    }
    let idx = (gid.y * u.tile.size.x + gid.x) * 4u;
    var r = input_px[idx];
    var g = input_px[idx + 1u];
    var b = input_px[idx + 2u];
    let a = input_px[idx + 3u];
    let op = u.ctrl.x;

    if (op == 0u) {
        r = 1.0 - r;
        g = 1.0 - g;
        b = 1.0 - b;
    } else if (op == 1u) {
        let threshold = clamp(u.p0.x, 0.0, 1.0);
        let v = select(0.0, 1.0, luma709(r, g, b) >= threshold);
        r = v;
        g = v;
        b = v;
    } else if (op == 2u) {
        let levels = clamp(u.p0.x, 2.0, 64.0);
        r = quantize_channel(r, levels);
        g = quantize_channel(g, levels);
        b = quantize_channel(b, levels);
    } else if (op == 3u) {
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
        let lum = luma709(r, g, b);
        r = lum + (r - lum) * sat_m;
        g = lum + (g - lum) * sat_m;
        b = lum + (b - lum) * sat_m;
        if (a <= 1e-6) {
            output_px[idx] = 0.0;
            output_px[idx + 1u] = 0.0;
            output_px[idx + 2u] = 0.0;
            output_px[idx + 3u] = a;
            return;
        }
        r = clamp(r, 0.0, 1.0);
        g = clamp(g, 0.0, 1.0);
        b = clamp(b, 0.0, 1.0);

        if (shadows != 0.0 || highlights != 0.0) {
            let y = luma709(r, g, b);
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
            let y = luma709(r, g, b);
            r = clamp(y + (r - y) * scale, 0.0, 1.0);
            g = clamp(y + (g - y) * scale, 0.0, 1.0);
            b = clamp(y + (b - y) * scale, 0.0, 1.0);
        }

        if (hue_deg != 0.0) {
            let hsl = rgb_to_hsl(r, g, b);
            var nh = hsl.x + hue_deg / 360.0;
            nh = nh - floor(nh);
            let rgb = hsl_to_rgb(nh, hsl.y, hsl.z);
            r = clamp(rgb.r, 0.0, 1.0);
            g = clamp(rgb.g, 0.0, 1.0);
            b = clamp(rgb.b, 0.0, 1.0);
        }
    } else if (op == 4u) {
        let mask = u.ctrl.y;
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
    } else if (op == 5u) {
        let ch = u.ctrl.y;
        if (ch == 0u) {
            r = lut1d_lookup(r);
        } else if (ch == 1u || ch == 4u) {
            g = lut1d_lookup(g);
        } else if (ch == 2u) {
            b = lut1d_lookup(b);
        } else {
            r = lut1d_lookup(r);
            g = lut1d_lookup(g);
            b = lut1d_lookup(b);
        }
    }

    output_px[idx] = r;
    output_px[idx + 1u] = g;
    output_px[idx + 2u] = b;
    output_px[idx + 3u] = a;
}
