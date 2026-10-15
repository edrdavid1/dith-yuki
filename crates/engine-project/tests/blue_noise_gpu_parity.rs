//! GPU blue_noise byte-identical to CPU reference.

use std::sync::atomic::AtomicBool;

use engine_project::algorithms::gpu_dither_cpu::{
    blue_noise_cpu, blue_noise_ranks, palette_rgba8_blue_noise, PaletteRgb, Rgba8, DEFAULT_SPREAD,
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

#[test]
#[ignore = "requires GPU adapter"]
fn blue_noise_gpu_matches_cpu_256() {
    let Some((device, queue, backend)) = engine_gpu::request_blocked_fs_device() else {
        eprintln!("skip: no GPU");
        return;
    };
    eprintln!("backend={backend}");
    let side = 256u32;
    let input = gray_ramp(side as usize, side as usize);
    let pal = [
        PaletteRgb { r: 0, g: 0, b: 0 },
        PaletteRgb {
            r: 255,
            g: 255,
            b: 255,
        },
        PaletteRgb {
            r: 128,
            g: 128,
            b: 128,
        },
    ];
    let ranks = blue_noise_ranks();
    let cpu = blue_noise_cpu(&input, side, side, &pal, ranks, DEFAULT_SPREAD).unwrap();
    let mut in_bytes = Vec::with_capacity(input.len() * 4);
    for p in &input {
        in_bytes.extend_from_slice(&[p.r, p.g, p.b, p.a]);
    }
    let pal_rgba = palette_rgba8_blue_noise(&pal);
    let cancel = AtomicBool::new(false);
    let gpu = engine_gpu::apply_blue_noise_gpu(
        &device,
        &queue,
        &in_bytes,
        side,
        side,
        &pal_rgba,
        ranks,
        DEFAULT_SPREAD,
        &cancel,
    )
    .expect("gpu");
    let mut cpu_b = Vec::with_capacity(cpu.len() * 4);
    for p in &cpu {
        cpu_b.extend_from_slice(&[p.r, p.g, p.b, p.a]);
    }
    assert_eq!(cpu_b, gpu);
}
