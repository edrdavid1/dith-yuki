// Palette nearest quantize — resident storage textures (Path B).
// Ports CPU palette_quantize::apply_nearest (Oklab → PaletteLut3D → palette RGB).

const HALO: u32 = 2u;
const CORE: u32 = 256u;

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct PaletteQuantUniforms {
    tile: TileUniforms,
    // x=lut_size, y=palette_len
    params: vec4<f32>,
}

struct PaletteMeta {
    // x=l_lo y=l_hi z=a_lo w=a_hi
    la: vec4<f32>,
    // x=b_lo y=b_hi
    b_pad: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: PaletteQuantUniforms;
@group(0) @binding(1) var tile_in: texture_2d<f32>;
@group(0) @binding(2) var tile_out: texture_storage_2d<rgba32float, write>;
@group(0) @binding(3) var<storage, read> pal_meta: PaletteMeta;
@group(0) @binding(4) var<storage, read> lut: array<u32>;
@group(0) @binding(5) var<storage, read> palette: array<vec4<f32>>;

// Björn Ottosson M1 / M2 (sRGB/Rec.709) — match engine_color::oklab.
fn linear_to_oklab(rgb: vec3<f32>) -> vec3<f32> {
    let r = clamp(rgb.r, 0.0, 1.0);
    let g = clamp(rgb.g, 0.0, 1.0);
    let b = clamp(rgb.b, 0.0, 1.0);
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
    let l_ = pow(l, 1.0 / 3.0);
    let m_ = pow(m, 1.0 / 3.0);
    let s_ = pow(s, 1.0 / 3.0);
    return vec3<f32>(
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    );
}

fn axis_index(v: f32, lo: f32, hi: f32, n: u32) -> u32 {
    let span = hi - lo;
    if (span <= 0.0) {
        return 0u;
    }
    let t = clamp((v - lo) / span, 0.0, 0.9999999);
    let i = u32(floor(t * f32(n)));
    return min(i, n - 1u);
}

fn lut_lookup(lab: vec3<f32>) -> u32 {
    let n = u32(u.params.x);
    let i = axis_index(lab.x, pal_meta.la.x, pal_meta.la.y, n);
    let j = axis_index(lab.y, pal_meta.la.z, pal_meta.la.w, n);
    let k = axis_index(lab.z, pal_meta.b_pad.x, pal_meta.b_pad.y, n);
    let flat = (i * n + j) * n + k;
    return lut[flat];
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= CORE || gid.y >= CORE) {
        return;
    }
    let px = i32(gid.x + HALO);
    let py = i32(gid.y + HALO);
    let rgba = textureLoad(tile_in, vec2<i32>(px, py), 0);
    let lab = linear_to_oklab(rgba.rgb);
    let idx = lut_lookup(lab);
    let pal_len = u32(u.params.y);
    let safe = min(idx, max(pal_len, 1u) - 1u);
    let c = palette[safe];
    textureStore(tile_out, vec2<i32>(px, py), vec4<f32>(c.rgb, rgba.a));
}
