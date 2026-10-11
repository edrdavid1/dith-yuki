//! GPU blocked_fs byte-identical to CPU reference (§5 / §6.2 of the GPU ED scheme).

use std::sync::atomic::AtomicBool;

use engine_project::algorithms::gpu_dither_cpu::{
    blocked_fs_cpu, PaletteRgb, Rgba8, CORE, PAD,
};

fn gray_ramp(w: usize, h: usize) -> Vec<Rgba8> {
    let mut v = Vec::with_capacity(w * h);
    let denom = (w.max(2) - 1) as u32;
    for _y in 0..h {
        for x in 0..w {
            let g = ((x as u32 * 255) / denom) as u8;
            v.push(Rgba8::new(g, g, g, 255));
        }
    }
    v
}

fn six_palette() -> Vec<PaletteRgb> {
    vec![
        PaletteRgb { r: 0, g: 0, b: 0 },
        PaletteRgb {
            r: 255,
            g: 255,
            b: 255,
        },
        PaletteRgb {
            r: 255,
            g: 0,
            b: 0,
        },
        PaletteRgb {
            r: 0,
            g: 255,
            b: 0,
        },
        PaletteRgb {
            r: 0,
            g: 0,
            b: 255,
        },
        PaletteRgb {
            r: 128,
            g: 128,
            b: 128,
        },
    ]
}

fn to_bytes(px: &[Rgba8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(px.len() * 4);
    for p in px {
        v.extend_from_slice(&[p.r, p.g, p.b, p.a]);
    }
    v
}

fn palette_rgba(pal: &[PaletteRgb]) -> Vec<[u8; 4]> {
    pal.iter()
        .map(|p| [p.r, p.g, p.b, 255])
        .collect()
}

fn parity_at(side: u32) {
    let Some((device, queue, backend)) = engine_gpu::request_blocked_fs_device() else {
        eprintln!("skip: no suitable GPU adapter");
        return;
    };
    eprintln!("backend={backend} side={side}");
    let input = gray_ramp(side as usize, side as usize);
    let pal = six_palette();
    let cpu = blocked_fs_cpu(&input, side, side, &pal, CORE, CORE, PAD).expect("cpu");
    let cancel = AtomicBool::new(false);
    let gpu = engine_gpu::apply_blocked_fs_gpu(
        &device,
        &queue,
        &to_bytes(&input),
        side,
        side,
        &palette_rgba(&pal),
        &cancel,
    )
    .expect("gpu");
    let cpu_b = to_bytes(&cpu);
    assert_eq!(cpu_b.len(), gpu.len());
    let mut mismatches = 0usize;
    for i in 0..cpu_b.len() {
        if cpu_b[i] != gpu[i] {
            mismatches += 1;
        }
    }
    assert_eq!(
        mismatches, 0,
        "GPU≠CPU on {side}²: {mismatches} byte mismatches"
    );
}

#[test]
#[ignore = "requires GPU adapter"]
fn blocked_fs_gpu_matches_cpu_3x3_tiles() {
    parity_at(CORE * 3); // 168
}

#[test]
#[ignore = "requires GPU adapter; slow"]
fn blocked_fs_gpu_matches_cpu_4k() {
    parity_at(4096);
}
