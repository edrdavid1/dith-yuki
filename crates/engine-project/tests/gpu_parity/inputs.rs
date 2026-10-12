use std::path::PathBuf;

use engine_project::algorithms::gpu_dither_cpu::Rgba8;
use image::RgbaImage;

use super::case::InputKind;
use super::rng::SplitMix64;

pub fn build_input(kind: &InputKind, w: u32, h: u32) -> Vec<Rgba8> {
    let wu = w as usize;
    let hu = h as usize;
    let n = wu * hu;
    match kind {
        InputKind::Solid { r, g, b, a } => vec![Rgba8::new(*r, *g, *b, *a); n],
        InputKind::GradientH => {
            let denom = (w.max(2) - 1) as u32;
            let mut v = Vec::with_capacity(n);
            for _y in 0..hu {
                for x in 0..wu {
                    let g = ((x as u32 * 255) / denom) as u8;
                    v.push(Rgba8::new(g, g, g, 255));
                }
            }
            v
        }
        InputKind::GradientV => {
            let denom = (h.max(2) - 1) as u32;
            let mut v = Vec::with_capacity(n);
            for y in 0..hu {
                let g = ((y as u32 * 255) / denom) as u8;
                for _x in 0..wu {
                    v.push(Rgba8::new(g, g, g, 255));
                }
            }
            v
        }
        InputKind::GradientDiag => {
            let denom = ((w + h).max(2) - 1) as u32;
            let mut v = Vec::with_capacity(n);
            for y in 0..hu {
                for x in 0..wu {
                    let g = (((x + y) as u32 * 255) / denom) as u8;
                    v.push(Rgba8::new(g, g, g, 255));
                }
            }
            v
        }
        InputKind::GradientRadial => {
            let cx = (w as f32 - 1.0) * 0.5;
            let cy = (h as f32 - 1.0) * 0.5;
            let max_r = (cx.hypot(cy)).max(1.0);
            let mut v = Vec::with_capacity(n);
            for y in 0..hu {
                for x in 0..wu {
                    let d = ((x as f32 - cx).hypot(y as f32 - cy) / max_r).clamp(0.0, 1.0);
                    let g = (d * 255.0) as u8;
                    v.push(Rgba8::new(g, g, g, 255));
                }
            }
            v
        }
        InputKind::RampR => ramp_channel(wu, hu, 0),
        InputKind::RampG => ramp_channel(wu, hu, 1),
        InputKind::RampB => ramp_channel(wu, hu, 2),
        InputKind::Checker => {
            let mut v = Vec::with_capacity(n);
            for y in 0..hu {
                for x in 0..wu {
                    let on = ((x + y) & 1) == 0;
                    let c = if on { 255 } else { 0 };
                    v.push(Rgba8::new(c, c, c, 255));
                }
            }
            v
        }
        InputKind::Noise { seed } => {
            let mut rng = SplitMix64::new(*seed);
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                v.push(Rgba8::new(rng.gen_u8(), rng.gen_u8(), rng.gen_u8(), 255));
            }
            v
        }
        InputKind::BwEdges => {
            let mut v = Vec::with_capacity(n);
            for y in 0..hu {
                for x in 0..wu {
                    let edge = x == 0 || y == 0 || x + 1 == wu || y + 1 == hu;
                    let c = if edge { 255 } else { 0 };
                    v.push(Rgba8::new(c, c, c, 255));
                }
            }
            v
        }
        InputKind::BwStripes => {
            let mut v = Vec::with_capacity(n);
            for _y in 0..hu {
                for x in 0..wu {
                    let c = if (x / 4) % 2 == 0 { 255 } else { 0 };
                    v.push(Rgba8::new(c, c, c, 255));
                }
            }
            v
        }
        InputKind::AllZero => vec![Rgba8::new(0, 0, 0, 255); n],
        InputKind::AllMax => vec![Rgba8::new(255, 255, 255, 255); n],
        InputKind::OutOfGamut => {
            let mut v = Vec::with_capacity(n);
            for y in 0..hu {
                for x in 0..wu {
                    let c = if ((x + y) & 1) == 0 { 0 } else { 255 };
                    v.push(Rgba8::new(c, c, c, 255));
                }
            }
            v
        }
        InputKind::AccumulatingError => vec![Rgba8::new(127, 127, 127, 255); n],
        InputKind::AlphaVarying => {
            let mut v = Vec::with_capacity(n);
            for y in 0..hu {
                for x in 0..wu {
                    let a = match (x + y) % 3 {
                        0 => 0,
                        1 => 128,
                        _ => 255,
                    };
                    let g = ((x as u32 * 255) / w.max(1)) as u8;
                    v.push(Rgba8::new(g, g, g, a));
                }
            }
            v
        }
        InputKind::Photo { name } => load_photo_resized(name, w, h),
    }
}

fn ramp_channel(w: usize, h: usize, ch: usize) -> Vec<Rgba8> {
    let mut v = Vec::with_capacity(w * h);
    for i in 0..(w * h) {
        let t = (i % 256) as u8;
        let mut rgb = [0u8; 3];
        rgb[ch] = t;
        v.push(Rgba8::new(rgb[0], rgb[1], rgb[2], 255));
    }
    v
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gpu_parity/photos")
}

fn load_photo_resized(name: &str, w: u32, h: u32) -> Vec<Rgba8> {
    let path = fixtures_dir().join(name);
    let img = image::open(&path)
        .unwrap_or_else(|e| panic!("open photo fixture {}: {e}", path.display()))
        .to_rgba8();
    let resized = image::imageops::resize(&img, w, h, image::imageops::FilterType::Nearest);
    rgba_image_to_pixels(&resized)
}

fn rgba_image_to_pixels(img: &RgbaImage) -> Vec<Rgba8> {
    img.pixels()
        .map(|p| Rgba8::new(p[0], p[1], p[2], p[3]))
        .collect()
}

pub fn photo_names() -> Vec<&'static str> {
    vec![
        "photo_sky.png",
        "photo_skin.png",
        "photo_edge.png",
        "photo_noise.png",
        "photo_grad.png",
    ]
}

pub fn quick_inputs(seed: u64) -> Vec<(String, InputKind)> {
    let mut v = Vec::new();
    for &(tag, r, g, b) in &[
        ("s0", 0u8, 0, 0),
        ("s1", 1, 1, 1),
        ("s127", 127, 127, 127),
        ("s128", 128, 128, 128),
        ("s254", 254, 254, 254),
        ("s255", 255, 255, 255),
    ] {
        v.push((tag.to_string(), InputKind::Solid { r, g, b, a: 255 }));
    }
    let mut rng = SplitMix64::new(seed ^ 0xA11CE);
    v.push((
        "srand".into(),
        InputKind::Solid {
            r: rng.gen_u8(),
            g: rng.gen_u8(),
            b: rng.gen_u8(),
            a: 255,
        },
    ));
    v.push(("grad_h".into(), InputKind::GradientH));
    v.push(("grad_v".into(), InputKind::GradientV));
    v.push(("grad_d".into(), InputKind::GradientDiag));
    v.push(("grad_r".into(), InputKind::GradientRadial));
    v.push(("noise0".into(), InputKind::Noise { seed }));
    v.push(("noise1".into(), InputKind::Noise { seed: seed ^ 0x55 }));
    // Critical divergence classes (must appear in quick inventory).
    v.push(("oog".into(), InputKind::OutOfGamut));
    v.push(("accum".into(), InputKind::AccumulatingError));
    v
}

pub fn full_inputs(seed: u64) -> Vec<(String, InputKind)> {
    let mut v = quick_inputs(seed);
    v.extend([
        ("ramp_r".into(), InputKind::RampR),
        ("ramp_g".into(), InputKind::RampG),
        ("ramp_b".into(), InputKind::RampB),
        ("checker".into(), InputKind::Checker),
        ("bw_edges".into(), InputKind::BwEdges),
        ("bw_stripes".into(), InputKind::BwStripes),
        ("all0".into(), InputKind::AllZero),
        ("all255".into(), InputKind::AllMax),
        ("oog".into(), InputKind::OutOfGamut),
        ("accum".into(), InputKind::AccumulatingError),
        ("alpha".into(), InputKind::AlphaVarying),
    ]);
    for name in photo_names() {
        v.push((
            format!("photo_{}", name.trim_end_matches(".png")),
            InputKind::Photo {
                name: name.to_string(),
            },
        ));
    }
    v
}

pub fn to_bytes(px: &[Rgba8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(px.len() * 4);
    for p in px {
        v.extend_from_slice(&[p.r, p.g, p.b, p.a]);
    }
    v
}

pub fn from_bytes(bytes: &[u8]) -> Vec<Rgba8> {
    bytes
        .chunks_exact(4)
        .map(|c| Rgba8::new(c[0], c[1], c[2], c[3]))
        .collect()
}
