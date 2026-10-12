use std::fs;
use std::path::{Path, PathBuf};

use engine_project::algorithms::gpu_dither_cpu::{Rgba8, CORE};
use image::{ImageBuffer, Rgba};

use super::case::ParityCase;
use super::inputs::to_bytes;

pub fn failures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("gpu_parity_failures")
}

pub struct Mismatch {
    pub x: u32,
    pub y: u32,
    pub cpu: [u8; 4],
    pub gpu: [u8; 4],
}

pub fn collect_mismatches(cpu: &[u8], gpu: &[u8], w: u32) -> Vec<Mismatch> {
    let mut out = Vec::new();
    let n = cpu.len().min(gpu.len()) / 4;
    for i in 0..n {
        let o = i * 4;
        if cpu[o..o + 4] != gpu[o..o + 4] {
            let x = (i as u32) % w;
            let y = (i as u32) / w;
            out.push(Mismatch {
                x,
                y,
                cpu: [cpu[o], cpu[o + 1], cpu[o + 2], cpu[o + 3]],
                gpu: [gpu[o], gpu[o + 1], gpu[o + 2], gpu[o + 3]],
            });
        }
    }
    out
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) {
    let img: ImageBuffer<Rgba<u8>, _> =
        ImageBuffer::from_raw(w, h, rgba.to_vec()).expect("png buffer");
    img.save(path).expect("save png");
}

pub fn write_failure_report(
    case: &ParityCase,
    input: &[Rgba8],
    cpu: &[u8],
    gpu: &[u8],
    kind: &str,
) {
    let dir = failures_root().join(&case.id);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("mkdir failure dir");

    let case_json = serde_json::to_string_pretty(case).expect("case json");
    fs::write(dir.join("case.json"), case_json).expect("case.json");

    let in_bytes = to_bytes(input);
    write_png(&dir.join("input.png"), case.width, case.height, &in_bytes);
    if cpu.len() == (case.width * case.height * 4) as usize {
        write_png(&dir.join("cpu.png"), case.width, case.height, cpu);
    }
    if gpu.len() == (case.width * case.height * 4) as usize {
        write_png(&dir.join("gpu.png"), case.width, case.height, gpu);
    }

    let mismatches = collect_mismatches(cpu, gpu, case.width);
    let mut diff = vec![0u8; (case.width * case.height * 4) as usize];
    for m in &mismatches {
        let o = ((m.y * case.width + m.x) * 4) as usize;
        diff[o] = 255;
        diff[o + 1] = 0;
        diff[o + 2] = 0;
        diff[o + 3] = 255;
    }
    write_png(&dir.join("diff.png"), case.width, case.height, &diff);

    let mut report = String::new();
    report.push_str(&format!("kind: {kind}\n"));
    report.push_str(&format!("case: {}\n", case.id));
    report.push_str(&format!("size: {}x{}\n", case.width, case.height));
    report.push_str(&format!("mismatched_pixels: {}\n", mismatches.len()));
    report.push_str("first_20:\n");
    for m in mismatches.iter().take(20) {
        let tx = m.x / CORE;
        let ty = m.y / CORE;
        let lx = m.x % CORE;
        let ly = m.y % CORE;
        let t = lx + 2 * ly;
        report.push_str(&format!(
            "  ({},{}) tile=({},{}) local=({},{}) wave_t={} cpu={:?} gpu={:?}\n",
            m.x, m.y, tx, ty, lx, ly, t, m.cpu, m.gpu
        ));
    }
    fs::write(dir.join("report.txt"), report).expect("report.txt");
    eprintln!("  wrote failure report → {}", dir.display());
}
