//! Offline sRGB-gamut shell in Oklab for Color Lab's PaletteVolumeViewer.
//!
//! Samples the six faces of the sRGB cube, converts each vertex with
//! `srgb_to_linear` + `linear_to_oklab` (same path as quantization / Track L IPC).
//! Positions are stored in scene order **(a, L, b)** = Three.js **(x, y, z)**.
//!
//! Runtime must load the checked-in JSON — do not rebuild this mesh on panel open.
//!
//! Regenerate from the repo root:
//! ```text
//! cargo run -p engine-color --bin gen-srgb-gamut-mesh -- \
//!   frontend/src/features/color-lab/assets/srgb-gamut-oklab.json
//! ```

use engine_color::palette::srgb_to_linear;
use engine_color::{linear_to_oklab, LinRgb};
use serde::Serialize;
use std::env;
use std::fs;
use std::path::PathBuf;

/// Samples per cube edge (inclusive). 16 → 15 segments, smooth enough for a faint shell.
const EDGE: u32 = 16;

#[derive(Serialize)]
struct GamutMesh {
    /// How to regenerate this file (also in the binary header).
    regenerate: &'static str,
    /// Scene mapping locked by Track L: X=a, Y=L (up), Z=b.
    axis_order: [&'static str; 3],
    /// Flat f32 triples: a, L, b, a, L, b, …
    positions: Vec<f32>,
    /// Triangle indices into `positions` (groups of 3 floats per vertex).
    indices: Vec<u32>,
}

fn srgb_u8_to_oklab_ab_l(r: u8, g: u8, b: u8) -> [f32; 3] {
    let lab = linear_to_oklab(LinRgb {
        r: srgb_to_linear(r),
        g: srgb_to_linear(g),
        b: srgb_to_linear(b),
    });
    [lab.a, lab.l, lab.b]
}

fn edge_u8(i: u32) -> u8 {
    ((i * 255) / (EDGE - 1)) as u8
}

/// Push one RGB-cube face (fixed channel) as a grid of Oklab vertices + triangles.
fn push_face(
    positions: &mut Vec<f32>,
    indices: &mut Vec<u32>,
    fixed_channel: usize,
    fixed_value: u8,
) {
    let base = (positions.len() / 3) as u32;
    for j in 0..EDGE {
        for i in 0..EDGE {
            let u = edge_u8(i);
            let v = edge_u8(j);
            let (r, g, b) = match fixed_channel {
                0 => (fixed_value, u, v),
                1 => (u, fixed_value, v),
                _ => (u, v, fixed_value),
            };
            let [a, l, bb] = srgb_u8_to_oklab_ab_l(r, g, b);
            positions.push(a);
            positions.push(l);
            positions.push(bb);
        }
    }
    for j in 0..EDGE - 1 {
        for i in 0..EDGE - 1 {
            let i0 = base + j * EDGE + i;
            let i1 = i0 + 1;
            let i2 = i0 + EDGE;
            let i3 = i2 + 1;
            indices.extend_from_slice(&[i0, i2, i1, i1, i2, i3]);
        }
    }
}

fn build_mesh() -> GamutMesh {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for channel in 0..3 {
        push_face(&mut positions, &mut indices, channel, 0);
        push_face(&mut positions, &mut indices, channel, 255);
    }
    GamutMesh {
        regenerate: "cargo run -p engine-color --bin gen-srgb-gamut-mesh -- frontend/src/features/color-lab/assets/srgb-gamut-oklab.json",
        axis_order: ["a", "L", "b"],
        positions,
        indices,
    }
}

fn main() {
    let out = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from("frontend/src/features/color-lab/assets/srgb-gamut-oklab.json")
        });
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).expect("create asset directory");
    }
    let mesh = build_mesh();
    let json = serde_json::to_string(&mesh).expect("serialize gamut mesh");
    fs::write(&out, json).unwrap_or_else(|e| panic!("write {}: {e}", out.display()));
    eprintln!(
        "wrote {} ({} verts, {} tris)",
        out.display(),
        mesh.positions.len() / 3,
        mesh.indices.len() / 3
    );
}
