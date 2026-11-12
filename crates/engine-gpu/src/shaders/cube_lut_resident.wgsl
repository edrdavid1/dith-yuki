// Adobe .cube 3D LUT — resident storage textures (Path B).
// Tetrahedral interpolation matches engine-color CubeLut3D::lookup.

const HALO: u32 = 2u;
const CORE: u32 = 256u;

struct TileUniforms {
    tile_offset: vec2<u32>,
    size: vec2<u32>,
}

struct CubeLutUniforms {
    tile: TileUniforms,
    // x=size, y=amount
    params: vec4<f32>,
    domain_min: vec4<f32>,
    domain_max: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: CubeLutUniforms;
@group(0) @binding(1) var tile_in: texture_2d<f32>;
@group(0) @binding(2) var tile_out: texture_storage_2d<rgba32float, write>;
@group(0) @binding(3) var<storage, read> cube: array<f32>;

fn sample_cube(r: u32, g: u32, b: u32, n: u32) -> vec3<f32> {
    let i = (r + n * (g + n * b)) * 3u;
    return vec3(cube[i], cube[i + 1u], cube[i + 2u]);
}

fn lerp3(c0: vec3<f32>, c1: vec3<f32>, c2: vec3<f32>, c3: vec3<f32>, t1: f32, t2: f32, t3: f32) -> vec3<f32> {
    return c0 + t1 * (c1 - c0) + t2 * (c2 - c1) + t3 * (c3 - c2);
}

fn domain_map(v: f32, lo: f32, hi: f32, maxv: f32) -> f32 {
    if (abs(hi - lo) < 1e-12) {
        return 0.0;
    }
    return clamp((v - lo) / (hi - lo), 0.0, 1.0) * maxv;
}

fn lookup_cube(rgb: vec3<f32>) -> vec3<f32> {
    let n = u32(u.params.x);
    let maxv = f32(n - 1u);
    let dmin = u.domain_min.xyz;
    let dmax = u.domain_max.xyz;
    let rf = domain_map(rgb.r, dmin.x, dmax.x, maxv);
    let gf = domain_map(rgb.g, dmin.y, dmax.y, maxv);
    let bf = domain_map(rgb.b, dmin.z, dmax.z, maxv);
    let r0 = u32(floor(rf));
    let g0 = u32(floor(gf));
    let b0 = u32(floor(bf));
    let r1 = min(r0 + 1u, n - 1u);
    let g1 = min(g0 + 1u, n - 1u);
    let b1 = min(b0 + 1u, n - 1u);
    let tr = rf - f32(r0);
    let tg = gf - f32(g0);
    let tb = bf - f32(b0);

    let c000 = sample_cube(r0, g0, b0, n);
    let c100 = sample_cube(r1, g0, b0, n);
    let c010 = sample_cube(r0, g1, b0, n);
    let c001 = sample_cube(r0, g0, b1, n);
    let c110 = sample_cube(r1, g1, b0, n);
    let c101 = sample_cube(r1, g0, b1, n);
    let c011 = sample_cube(r0, g1, b1, n);
    let c111 = sample_cube(r1, g1, b1, n);

    if (tr > tg) {
        if (tg > tb) {
            return lerp3(c000, c100, c110, c111, tr, tg, tb);
        } else if (tr > tb) {
            return lerp3(c000, c100, c101, c111, tr, tb, tg);
        } else {
            return lerp3(c000, c001, c101, c111, tb, tr, tg);
        }
    } else if (tb > tg) {
        return lerp3(c000, c001, c011, c111, tb, tg, tr);
    } else if (tb > tr) {
        return lerp3(c000, c010, c011, c111, tg, tb, tr);
    } else {
        return lerp3(c000, c010, c110, c111, tg, tr, tb);
    }
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= CORE || gid.y >= CORE) {
        return;
    }
    let px = i32(gid.x + HALO);
    let py = i32(gid.y + HALO);
    let rgba = textureLoad(tile_in, vec2<i32>(px, py), 0);
    let amount = u.params.y;
    if (amount <= 0.0 || u.params.x < 2.0) {
        textureStore(tile_out, vec2<i32>(px, py), rgba);
        return;
    }
    let mapped = lookup_cube(rgba.rgb);
    var out = rgba;
    if (amount >= 1.0) {
        out = vec4(mapped, rgba.a);
    } else {
        out = vec4(rgba.rgb + (mapped - rgba.rgb) * amount, rgba.a);
    }
    textureStore(tile_out, vec2<i32>(px, py), out);
}
