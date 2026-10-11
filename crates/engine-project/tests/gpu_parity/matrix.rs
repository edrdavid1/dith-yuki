use super::case::{Algorithm, InputKind, Params, ParityCase};
use super::inputs::full_inputs;
use super::palettes::{full_palettes, quick_palettes, NamedPalette};
use super::rng::SplitMix64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParityMode {
    Quick,
    /// Deterministic matrix only (no random cases).
    Full,
    /// Random cases only (`PARITY_ITERS`, `PARITY_SEED`).
    Random,
    /// Deprecated convenience: same as Full then Random in one process.
    /// Prefer separate `full` and `random` runs.
    Nightly,
}

impl ParityMode {
    pub fn from_env() -> Self {
        match std::env::var("PARITY_MODE")
            .unwrap_or_else(|_| "quick".into())
            .to_ascii_lowercase()
            .as_str()
        {
            "full" => Self::Full,
            "random" => Self::Random,
            "nightly" => Self::Nightly,
            _ => Self::Quick,
        }
    }
}

fn tiny_sizes() -> Vec<u32> {
    vec![1, 2, 3, 7, 8, 9]
}

fn around_56() -> Vec<u32> {
    vec![55, 56, 57, 111, 112, 113, 167, 168, 169]
}

fn around_64() -> Vec<u32> {
    vec![63, 64, 65, 127, 128, 129]
}

fn around_128() -> Vec<u32> {
    vec![127, 128, 129, 255, 256, 257]
}

fn odds() -> Vec<u32> {
    vec![97, 101, 251, 509, 1023, 1025]
}

fn extremes() -> Vec<(u32, u32)> {
    vec![(1, 4096), (4096, 1), (8192, 8), (8, 8192)]
}

fn typical() -> Vec<(u32, u32)> {
    vec![(640, 480), (1920, 1080), (3840, 2160)]
}

fn large_full() -> Vec<(u32, u32)> {
    vec![(8192, 8192), (16384, 16384)]
}

fn strip_sizes() -> Vec<(u32, u32)> {
    let ns = [1u32, 8, 56, 64, 1000];
    let mut v = Vec::new();
    for n in ns {
        v.push((1, n));
        v.push((n, 1));
    }
    v
}

/// Unique sizes ≤129 for full cartesian pairs.
fn small_axis_values() -> Vec<u32> {
    let mut s: Vec<u32> = tiny_sizes();
    s.extend(around_56().into_iter().filter(|&x| x <= 129));
    s.extend(around_64());
    s.extend(around_128().into_iter().filter(|&x| x <= 129));
    s.extend(odds().into_iter().filter(|&x| x <= 129));
    s.extend([1, 8, 56, 64]);
    s.sort_unstable();
    s.dedup();
    s
}

fn filter_substr() -> Option<String> {
    std::env::var("PARITY_FILTER")
        .ok()
        .filter(|s| !s.is_empty())
}

fn case_tags(
    w: u32,
    h: u32,
    pal: &NamedPalette,
    input_tag: &str,
    input: &InputKind,
) -> Vec<String> {
    let mut tags = Vec::new();
    match pal.tag {
        "ptie" => tags.push("tie_palette".into()),
        "p2same" => tags.push("identical_palette".into()),
        "p2close" => tags.push("close_palette".into()),
        "pog" => tags.push("oog_palette".into()),
        "p1" => tags.push("palette_1".into()),
        "p2" => tags.push("palette_2".into()),
        _ => {}
    }
    if pal.tag.starts_with('p') {
        if let Ok(n) = pal.tag[1..].parse::<u32>() {
            if n >= 255 {
                tags.push("palette_ge_255".into());
            }
        }
    }
    match input {
        InputKind::OutOfGamut => tags.push("oog_input".into()),
        InputKind::AccumulatingError => tags.push("accum_error".into()),
        InputKind::AlphaVarying => tags.push("alpha_varying".into()),
        InputKind::Solid { r, g, b, .. } if *r == 128 && *g == 128 && *b == 128 => {
            tags.push("mid_gray_solid".into());
        }
        _ => {}
    }
    if input_tag.contains("photo") {
        tags.push("photo".into());
    }
    let edge = matches!(
        w,
        1 | 55 | 56 | 57 | 63 | 64 | 65 | 127 | 128 | 129
    ) || matches!(
        h,
        1 | 55 | 56 | 57 | 63 | 64 | 65 | 127 | 128 | 129
    );
    if edge {
        tags.push("edge_size".into());
    }
    if w <= 9 && h <= 9 {
        tags.push("tiny".into());
    }
    if w == 1 || h == 1 {
        tags.push("strip_1px".into());
    }
    if pal.tag == "ptie" && matches!(input, InputKind::Solid { .. } | InputKind::AccumulatingError) {
        tags.push("tie_exercise".into());
    }
    tags
}

fn push_case(
    out: &mut Vec<ParityCase>,
    algo: Algorithm,
    w: u32,
    h: u32,
    pal: &NamedPalette,
    input_tag: &str,
    input: InputKind,
    seed: u64,
    group: &str,
    params: Params,
    filter: &Option<String>,
) {
    let id = ParityCase::make_id(algo, w, h, pal.tag, input_tag, seed);
    if let Some(f) = filter {
        if !id.contains(f) {
            return;
        }
    }
    let tags = case_tags(w, h, pal, input_tag, &input);
    out.push(ParityCase {
        id,
        algorithm: algo,
        width: w,
        height: h,
        palette: (pal.colors)(),
        input,
        seed,
        params,
        group: group.into(),
        tags,
    });
}

/// Print §9-style coverage inventory for a built matrix.
pub fn print_coverage_table(cases: &[ParityCase]) {
    use std::collections::BTreeMap;
    let mut by_group: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_tag: BTreeMap<&str, usize> = BTreeMap::new();
    let mut bfs = 0usize;
    let mut bn = 0usize;
    for c in cases {
        *by_group.entry(c.group.as_str()).or_insert(0) += 1;
        match c.algorithm {
            Algorithm::BlockedFs => bfs += 1,
            Algorithm::BlueNoise => bn += 1,
        }
        for t in &c.tags {
            *by_tag.entry(t.as_str()).or_insert(0) += 1;
        }
    }
    eprintln!("\n=== coverage inventory ({} cases) ===", cases.len());
    eprintln!("algorithm\tblocked_fs={bfs}\tblue_noise={bn}");
    eprintln!("group\tcount");
    for (g, n) in &by_group {
        eprintln!("{g}\t{n}");
    }
    eprintln!("tag\tcount");
    for key in [
        "tie_palette",
        "tie_exercise",
        "identical_palette",
        "close_palette",
        "oog_palette",
        "oog_input",
        "accum_error",
        "mid_gray_solid",
        "alpha_varying",
        "palette_1",
        "palette_2",
        "palette_ge_255",
        "edge_size",
        "tiny",
        "strip_1px",
        "photo",
    ] {
        let n = by_tag.get(key).copied().unwrap_or(0);
        eprintln!("{key}\t{n}");
    }
    eprintln!("=== end inventory ===\n");
}

fn expand_product(
    out: &mut Vec<ParityCase>,
    sizes: &[(u32, u32)],
    pals: &[NamedPalette],
    inputs: &[(String, InputKind)],
    algos: &[Algorithm],
    seed: u64,
    group: &str,
    params: &Params,
    filter: &Option<String>,
) {
    for &(w, h) in sizes {
        for pal in pals {
            for (itag, ikind) in inputs {
                for &algo in algos {
                    let mut p = params.clone();
                    if algo == Algorithm::BlueNoise {
                        p.spread = params.spread;
                    }
                    push_case(
                        out,
                        algo,
                        w,
                        h,
                        pal,
                        itag,
                        ikind.clone(),
                        seed,
                        group,
                        p,
                        filter,
                    );
                }
            }
        }
    }
}

pub fn build_matrix(mode: ParityMode, seed: u64) -> Vec<ParityCase> {
    let mut out = Vec::new();
    let algos = [Algorithm::BlockedFs, Algorithm::BlueNoise];
    let filter = filter_substr();

    match mode {
        ParityMode::Quick => {
            // CI subset: every critical category, few minutes — not the full matrix.
            // Full is for pre-release / overnight only.
            append_quick_ci(&mut out, seed, &algos, &filter);
        }
        ParityMode::Full | ParityMode::Nightly => {
            let small: Vec<(u32, u32)> = {
                let axes = small_axis_values();
                let mut pairs = Vec::new();
                for &w in &axes {
                    for &h in &axes {
                        pairs.push((w, h));
                    }
                }
                pairs
            };
            let boundary: Vec<(u32, u32)> = {
                let axes = [1u32, 2, 8, 9, 55, 56, 57, 63, 64, 65, 127, 128, 129];
                let mut pairs = Vec::new();
                for &w in &axes {
                    for &h in &axes {
                        pairs.push((w, h));
                    }
                }
                pairs
            };
            append_full_deterministic(&mut out, seed, &algos, &small, &boundary, &filter);
            if mode == ParityMode::Nightly {
                eprintln!(
                    "note: PARITY_MODE=nightly combines full+random; prefer separate runs:\n\
                     \tPARITY_MODE=full …\n\
                     \tPARITY_MODE=random PARITY_ITERS=… PARITY_SEED=…"
                );
                append_random_cases(&mut out, seed, &filter);
            }
        }
        ParityMode::Random => {
            append_random_cases(&mut out, seed, &filter);
        }
    }

    if filter.is_some() {
        eprintln!("PARITY_FILTER={filter:?} → {} cases", out.len());
    }

    out
}

/// Compact matrix for every commit / CI: covers ties, OOG, identical palettes,
/// edge sizes, strips, mid-gray, both algorithms — without the full cartesian.
fn append_quick_ci(
    out: &mut Vec<ParityCase>,
    seed: u64,
    algos: &[Algorithm],
    filter: &Option<String>,
) {
    let default_params = Params::default();
    let pals = quick_palettes(); // includes p1/p2/p4/p16/p256/p2same/ptie/pog
    let inputs: Vec<(String, InputKind)> = [
        (
            "s128",
            InputKind::Solid {
                r: 128,
                g: 128,
                b: 128,
                a: 255,
            },
        ),
        ("grad_h", InputKind::GradientH),
        ("grad_d", InputKind::GradientDiag),
        ("noise0", InputKind::Noise { seed }),
        ("oog", InputKind::OutOfGamut),
        ("accum", InputKind::AccumulatingError),
        ("checker", InputKind::Checker),
    ]
    .into_iter()
    .map(|(t, k)| (t.into(), k))
    .collect();

    let sizes: Vec<(u32, u32)> = vec![
        (1, 1),
        (8, 8),
        (9, 9),
        (55, 56),
        (56, 56),
        (57, 57),
        (63, 64),
        (64, 64),
        (127, 128),
        (128, 128),
        (129, 129),
        (1, 64),
        (64, 1),
        (1, 1000),
        (168, 168),
        (256, 256),
    ];

    expand_product(
        out,
        &sizes,
        &pals,
        &inputs,
        algos,
        seed,
        "quick_ci",
        &default_params,
        filter,
    );

    // Param smoke (GPU-valid only: pad fixed at 8).
    let param_pal = *pals.iter().find(|p| p.tag == "p2").expect("p2");
    for spread in [0u32, 256] {
        let mut p = Params::default();
        p.spread = spread;
        push_case(
            out,
            Algorithm::BlueNoise,
            64,
            64,
            &param_pal,
            "grad_h",
            InputKind::GradientH,
            seed,
            "quick_params",
            p,
            filter,
        );
    }
}

fn append_full_deterministic(
    out: &mut Vec<ParityCase>,
    seed: u64,
    algos: &[Algorithm],
    small: &[(u32, u32)],
    boundary: &[(u32, u32)],
    filter: &Option<String>,
) {
    let default_params = Params::default();
    let pals = full_palettes();
    let inputs = full_inputs(seed);

    let base_pal = *pals.iter().find(|p| p.tag == "p2").unwrap_or(&pals[0]);
    expand_product(
        out,
        small,
        &[base_pal],
        &[("grad_h".into(), InputKind::GradientH)],
        algos,
        seed,
        "full_sizes",
        &default_params,
        filter,
    );

    expand_product(
        out,
        boundary,
        &pals,
        &inputs,
        algos,
        seed,
        "full_small",
        &default_params,
        filter,
    );

    let mut cover: Vec<(u32, u32)> = Vec::new();
    cover.extend(strip_sizes());
    for s in around_56()
        .into_iter()
        .chain(around_64())
        .chain(around_128())
        .chain(odds())
    {
        if s > 129 {
            cover.push((s, s));
            cover.push((s, 56));
            cover.push((56, s));
            cover.push((s, 128));
            cover.push((128, s));
        }
    }
    cover.extend(extremes());
    cover.extend(typical());
    cover.sort_unstable();
    cover.dedup();

    let large_pals: Vec<NamedPalette> = full_palettes()
        .into_iter()
        .filter(|p| matches!(p.tag, "p2" | "p16" | "p256" | "ptie" | "pog" | "p257"))
        .collect();
    let large_inputs: Vec<(String, InputKind)> = full_inputs(seed)
        .into_iter()
        .filter(|(t, _)| {
            matches!(
                t.as_str(),
                "grad_h"
                    | "grad_d"
                    | "noise0"
                    | "checker"
                    | "accum"
                    | "oog"
                    | "alpha"
                    | "s127"
                    | "photo_grad"
            )
        })
        .collect();

    // Narrow strips (1×N / N×1) — required zero-skip group.
    let strip_only: Vec<(u32, u32)> = strip_sizes();
    expand_product(
        out,
        &strip_only,
        &large_pals,
        &large_inputs,
        algos,
        seed,
        "full_strips",
        &default_params,
        filter,
    );

    // Remaining cover without duplicating pure strips already added.
    let strip_set: std::collections::HashSet<(u32, u32)> = strip_only.into_iter().collect();
    let cover_rest: Vec<(u32, u32)> = cover
        .into_iter()
        .filter(|p| !strip_set.contains(p))
        .collect();
    expand_product(
        out,
        &cover_rest,
        &large_pals,
        &large_inputs,
        algos,
        seed,
        "full_cover",
        &default_params,
        filter,
    );

    let param_sizes = [(8u32, 8u32), (56, 56), (64, 64), (168, 168), (256, 256)];
    let param_pal = *quick_palettes()
        .iter()
        .find(|p| p.tag == "p2")
        .expect("p2");
    let param_input = InputKind::GradientH;
    for &(w, h) in &param_sizes {
        for pixel_size in [1u32, 2, 3, 4, 8] {
            let mut p = Params::default();
            p.pixel_size = pixel_size;
            push_case(
                out,
                Algorithm::BlockedFs,
                w,
                h,
                &param_pal,
                "grad_h",
                param_input.clone(),
                seed,
                "params_ps",
                p,
                filter,
            );
        }
        for spread in [0u32, 64, 256, 512] {
            let mut p = Params::default();
            p.spread = spread;
            push_case(
                out,
                Algorithm::BlueNoise,
                w,
                h,
                &param_pal,
                "grad_h",
                param_input.clone(),
                seed,
                "params_spread",
                p,
                filter,
            );
        }
        // GPU warm-up is fixed at PAD=8 — do not generate pad≠8 (those only skip).
        let mut p = Params::default();
        p.pad = 8;
        push_case(
            out,
            Algorithm::BlockedFs,
            w,
            h,
            &param_pal,
            "grad_h",
            param_input.clone(),
            seed,
            "params_pad",
            p,
            filter,
        );
    }

    expand_product(
        out,
        &large_full(),
        &[large_pals[0]],
        &[("grad_h".to_string(), InputKind::GradientH)],
        algos,
        seed,
        "full_large",
        &default_params,
        filter,
    );
}

fn append_random_cases(out: &mut Vec<ParityCase>, seed: u64, filter: &Option<String>) {
    let n: u32 = std::env::var("PARITY_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20_000);
    eprintln!("random cases: N={n} seed={seed:#x}");
    let mut rng = SplitMix64::new(seed);
    let all_pals = full_palettes();
    for i in 0..n {
        let w = rng.gen_range_u32(1, 2048);
        let h = rng.gen_range_u32(1, 2048);
        let (w, h) = if (w as u64) * (h as u64) > 4_000_000 {
            let s = rng.gen_range_u32(1, 1024);
            (s, s)
        } else {
            (w, h)
        };
        let pal = rng.choose(&all_pals);
        let algo = if rng.next_u64() & 1 == 0 {
            Algorithm::BlockedFs
        } else {
            Algorithm::BlueNoise
        };
        let input_seed = rng.next_u64();
        let ikind = match rng.gen_range_u32(0, 8) {
            0 => InputKind::Solid {
                r: rng.gen_u8(),
                g: rng.gen_u8(),
                b: rng.gen_u8(),
                a: 255,
            },
            1 => InputKind::GradientH,
            2 => InputKind::GradientDiag,
            3 => InputKind::Checker,
            4 => InputKind::Noise { seed: input_seed },
            5 => InputKind::AccumulatingError,
            6 => InputKind::AlphaVarying,
            _ => InputKind::OutOfGamut,
        };
        let itag = format!("rand{i}");
        let mut params = Params::default();
        if algo == Algorithm::BlueNoise {
            params.spread = [0u32, 64, 256, 512][rng.gen_range_u32(0, 3) as usize];
        }
        let case_seed = rng.next_u64();
        push_case(
            out,
            algo,
            w,
            h,
            pal,
            &itag,
            ikind,
            case_seed,
            "random",
            params,
            filter,
        );
    }
}
