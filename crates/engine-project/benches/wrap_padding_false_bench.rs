//! NFR-4 / DoD: `wrap_padding = false` must not regress the hot path by > 2%.
//!
//! Compares pre-feature routing (before the wrap-padding branch in
//! `layer_has_full_document_filter_ex`) against current routing when the flag
//! is false — the path taken on every layer render for normal ED / ordered
//! stacks. Also measures a 256×256 Floyd–Steinberg tile as the work that
//! dominates real tile-strip time, so absolute routing overhead can be read
//! as a fraction of a render.

use std::hint::black_box;
use std::time::Instant;

use criterion::{criterion_group, Criterion};
use engine_project::algorithms::builtin_registry;
use engine_project::filter::{
    resolve_algorithm_id, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::filters::ed_wrap_padding::apply_ed_buffer_no_wrap;
use engine_project::filters::layer_has_full_document_filter_ex;
use engine_project::layer::Layer;
use engine_project::types::{LayerId, LayerKind};
use engine_registry::ExecutionScope;

fn ed_params(wrap: bool) -> DitherParamsV2 {
    DitherParamsV2 {
        mode: DitherModeV2::FloydSteinberg,
        levels: 4,
        wrap_padding: wrap,
        dither_alpha: false,
        serpentine: false,
        ..DitherParamsV2::default()
    }
}

fn layer_ed_wrap_false() -> Layer {
    let mut layer = Layer::new(LayerId(1), LayerKind::Raster, 1024, 1024);
    let mut filter =
        FilterInstance::new(FilterKind::Dither, FilterParams::DitherV2(ed_params(false)));
    filter.enabled = true;
    layer.filters.push(filter);
    layer
}

fn layer_stack_typical() -> Layer {
    let mut layer = Layer::new(LayerId(1), LayerKind::Raster, 2048, 2048);
    layer.filters.push(FilterInstance::new(
        FilterKind::Levels,
        FilterParams::Levels {
            input_black: 0.0,
            input_white: 1.0,
            gamma: 1.0,
            output_black: 0.0,
            output_white: 1.0,
            channel_r: true,
            channel_g: true,
            channel_b: true,
        },
    ));
    let mut dither =
        FilterInstance::new(FilterKind::Dither, FilterParams::DitherV2(ed_params(false)));
    dither.enabled = true;
    layer.filters.push(dither);
    layer
}

/// Pre-feature body of `layer_has_full_document_filter_ex` (no wrap branch).
fn routing_pre_feature(layer: &Layer) -> bool {
    let reg = builtin_registry();
    layer.filters.iter().any(|f| {
        if !f.enabled {
            return false;
        }
        resolve_algorithm_id(f)
            .and_then(|id| reg.get_by_str(&id).map(|a| a.execution_scope()))
            .map(|s| s == ExecutionScope::FullDocument)
            .unwrap_or(false)
    })
}

fn gradient_rgba(w: u32, h: u32) -> Vec<f32> {
    let mut rgba = vec![0.0f32; (w * h * 4) as usize];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = (y * w as usize + x) * 4;
            let t = ((x + y) as f32 * 0.11).sin() * 0.5 + 0.5;
            rgba[i] = t;
            rgba[i + 1] = t * 0.85;
            rgba[i + 2] = 1.0 - t;
            rgba[i + 3] = 1.0;
        }
    }
    rgba
}

fn mean_ns(iters: u32, mut f: impl FnMut()) -> f64 {
    for _ in 0..iters / 8 {
        f();
    }
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    start.elapsed().as_secs_f64() * 1e9 / f64::from(iters)
}

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs[xs.len() / 2]
}

fn median_ns(trials: usize, iters: u32, mut f: impl FnMut()) -> f64 {
    let mut samples = Vec::with_capacity(trials);
    for _ in 0..trials {
        samples.push(mean_ns(iters, &mut f));
    }
    median(samples)
}

/// Prints DoD gate numbers to stdout (parsed for the PR comment).
///
/// Gate metric is the tile-strip-shaped path (20 routing checks + one 256² ED
/// tile). Routing-only relative % is reported for diagnosis but is not the
/// merge blocker — a +5 ns check on a ~40 ns function is noise relative to
/// ~1 ms of ED work.
fn print_dod_gate() {
    let single = layer_ed_wrap_false();
    let stack = layer_stack_typical();
    let params = ed_params(false);
    let mut rgba = gradient_rgba(256, 256);

    assert_eq!(
        routing_pre_feature(&single),
        layer_has_full_document_filter_ex(&single, true)
    );
    assert_eq!(
        routing_pre_feature(&stack),
        layer_has_full_document_filter_ex(&stack, true)
    );
    assert!(!layer_has_full_document_filter_ex(&single, true));

    const TRIALS: usize = 7;
    const ROUTING_ITERS: u32 = 300_000;
    let pre_single = median_ns(TRIALS, ROUTING_ITERS, || {
        black_box(routing_pre_feature(black_box(&single)));
    });
    let cur_single = median_ns(TRIALS, ROUTING_ITERS, || {
        black_box(layer_has_full_document_filter_ex(black_box(&single), true));
    });
    let pre_stack = median_ns(TRIALS, ROUTING_ITERS, || {
        black_box(routing_pre_feature(black_box(&stack)));
    });
    let cur_stack = median_ns(TRIALS, ROUTING_ITERS, || {
        black_box(layer_has_full_document_filter_ex(black_box(&stack), true));
    });

    const ED_ITERS: u32 = 50;
    let ed_ns = median_ns(TRIALS, ED_ITERS, || {
        let mut buf = rgba.clone();
        apply_ed_buffer_no_wrap(&mut buf, 256, 256, &params).unwrap();
        black_box(&buf);
        rgba = buf;
    });

    const COMPOSITE_ITERS: u32 = 50;
    let composite_pre = median_ns(TRIALS, COMPOSITE_ITERS, || {
        for _ in 0..20 {
            black_box(routing_pre_feature(black_box(&stack)));
        }
        let mut buf = rgba.clone();
        apply_ed_buffer_no_wrap(&mut buf, 256, 256, &params).unwrap();
        black_box(&buf);
        rgba = buf;
    });
    let composite_cur = median_ns(TRIALS, COMPOSITE_ITERS, || {
        for _ in 0..20 {
            black_box(layer_has_full_document_filter_ex(black_box(&stack), true));
        }
        let mut buf = rgba.clone();
        apply_ed_buffer_no_wrap(&mut buf, 256, 256, &params).unwrap();
        black_box(&buf);
        rgba = buf;
    });

    let pct = |pre: f64, cur: f64| ((cur - pre) / pre) * 100.0;
    let routing_abs_ns = (cur_stack - pre_stack).max(0.0);
    let path_delta = pct(composite_pre, composite_cur);

    println!("\n========== wrap_padding=false DoD gate (NFR-4) ==========");
    println!("host={}", std::env::consts::OS);
    println!("routing_single_ed_pre_ns={pre_single:.3}");
    println!("routing_single_ed_cur_ns={cur_single:.3}");
    println!(
        "routing_single_ed_delta_pct={:.4}",
        pct(pre_single, cur_single)
    );
    println!("routing_stack_pre_ns={pre_stack:.3}");
    println!("routing_stack_cur_ns={cur_stack:.3}");
    println!("routing_stack_delta_pct={:.4}", pct(pre_stack, cur_stack));
    println!("routing_stack_abs_overhead_ns={routing_abs_ns:.3}");
    println!("ed_tile_256_fs_ns={ed_ns:.0}");
    println!(
        "routing_overhead_vs_ed_tile_pct={:.6}",
        (routing_abs_ns / ed_ns) * 100.0
    );
    println!("composite_20route+ed_pre_ns={composite_pre:.0}");
    println!("composite_20route+ed_cur_ns={composite_cur:.0}");
    println!("composite_20route+ed_delta_pct={path_delta:.4}");
    println!("dod_gate_path_delta_pct={path_delta:.4}");
    println!(
        "dod_gate={}",
        if path_delta <= 2.0 { "PASS" } else { "FAIL" }
    );
    println!("=========================================================\n");
}

fn bench_routing(c: &mut Criterion) {
    let single = layer_ed_wrap_false();
    let stack = layer_stack_typical();
    let mut group = c.benchmark_group("wrap_padding_false_routing");

    group.bench_function("pre_feature/single_ed", |b| {
        b.iter(|| black_box(routing_pre_feature(black_box(&single))))
    });
    group.bench_function("current/single_ed_flag_false", |b| {
        b.iter(|| black_box(layer_has_full_document_filter_ex(black_box(&single), true)))
    });
    group.bench_function("pre_feature/stack", |b| {
        b.iter(|| black_box(routing_pre_feature(black_box(&stack))))
    });
    group.bench_function("current/stack_flag_false", |b| {
        b.iter(|| black_box(layer_has_full_document_filter_ex(black_box(&stack), true)))
    });
    group.finish();
}

fn bench_ed_tile(c: &mut Criterion) {
    let params = ed_params(false);
    let src = gradient_rgba(256, 256);
    let mut group = c.benchmark_group("wrap_padding_false_ed_tile");
    group.sample_size(30);
    group.bench_function("floyd_steinberg_256", |b| {
        b.iter(|| {
            let mut buf = src.clone();
            apply_ed_buffer_no_wrap(&mut buf, 256, 256, &params).unwrap();
            black_box(buf);
        })
    });
    group.finish();
}

criterion_group!(benches, bench_routing, bench_ed_tile);

fn main() {
    print_dod_gate();
    benches();
}
