# How to add an algorithm

This is the checklist for adding a **built-in** filter algorithm. New algorithms
register themselves; the tile apply path, GPU eligibility signal, schema-driven
settings panel, and document load/save go through `AlgorithmRegistry`. Do not add
new arms to the old `FilterKind` / `DitherModeV2` dispatch matches.

`FilterKind` and `DitherModeV2` remain **legacy serde aliases** for on-disk
`FilterInstanceFile.kind` and `DitherParamsV2.mode`. New code uses `AlgorithmId`.

## Step 1 — Choose a stable `AlgorithmId`

Rules:

- `snake_case`, ASCII, no spaces.
- **Never rename** an ID after it ships. Display names may change; IDs must not.
- Do not reuse a retired ID.

Check `ALGORITHM_ID_REGISTRY.txt` at the repo root for conflicts.

## Step 2 — Add the ID to `ALGORITHM_ID_REGISTRY.txt`

Insert the ID in **alphabetical** order, one ID per line. CI test
`algorithm_id_registry_matches_txt` fails if the file and `register_all()` disagree.

Also update the expected list in `crates/engine-project/tests/registry_unit.rs`
(`algorithm_id_registry_txt_contains_expected_ids`) and the `register_all_ids_unique`
count.

## Step 3 — Implement `FilterAlgorithm`

Create `crates/engine-project/src/algorithms/<id>.rs`. Copy the worked example
below (`PassThroughAlgorithm`) and replace the identity + `apply` body.

Required methods:

| Method | Notes |
|---|---|
| `id()` | `AlgorithmId::new("your_id")` — same string as the txt file |
| `display_name()` | UI label; may change later |
| `apply()` | In-place on `tile`. Deserialize `params` with `serde_json::from_value`. Recover `FilterContext` via `FilterContext::from_ctx(ctx)` when you need document/caches. Copy to a scratch tile first if the kernel cannot alias src/dst (`algorithms/scratch.rs`). |
| `gpu_eligibility()` | `Eligible` or `Cpu(CpuCheckpointKind::…)`. The registry does **not** build GPU passes (Invariant 4). |
| `param_schema()` | Static `&[ParamField]` for `AlgorithmSettingsPanel`. Same slice every call. |
| `schema_version()` | Start at `1`. Increment only on breaking param changes. |
| `category()` | `Dithering`, `Glitch`, `ColorAdjust`, `Stylize`, or `Palette` |
| `requires_full_row()` | Default `false`. Error diffusion returns `true`. |
| `migrate_params()` | Default no-op. Must be pure and idempotent. |

Wire the module in `crates/engine-project/src/algorithms/mod.rs`:

```rust
mod your_id;
```

## Step 4 — Register in `register_all()`

In `crates/engine-project/src/algorithms/mod.rs`:

```rust
registry.register(Box::new(your_id::YourAlgo));
```

`builtin_registry()` calls `register_all` once. Duplicate IDs `debug_assert` in
`AlgorithmRegistry::register`.

## Step 5 — Parity test

Add `crates/engine-project/tests/registry_parity_<id>.rs`. Existing tests under
`tests/registry_parity_*.rs` use `tests/support/parity_harness.rs`.

Assert byte-identical output versus the kernel you wrap (or a hand-checked
oracle for new effects). GPU-eligible algorithms should also assert
`gpu_eligibility` for representative params.

## Step 6 — Parameter schema / UI

`param_schema()` is enough for the settings panel. Run `cargo tauri dev`, add
the effect from the category list, and confirm sliders/checkboxes/dropdowns
render. Empty schema (`&[]`) shows a panel with no controls.

Add a `tests/fixtures/migration/<id>_v1.json` `FilterInstanceFile` with
`schema_version: 1` so `migration_corpus` keeps covering load.

## Step 7 — GPU-eligible algorithms only

If `gpu_eligibility` returns `Eligible`, also add closed-enum variants:

1. `GraphLayerFilter`, `GpuPipelineKey`, and `GpuPass` in
   `crates/engine-gpu/src/graph/types.rs`
2. A case in `build_gpu_layer_filter()` in
   `crates/engine-project/src/filters/gpu_graph.rs`

`filter_to_spec` already maps `Eligible` → `build_gpu_layer_filter`. CPU-only
algorithms skip this step.

## Worked example: `PassThroughAlgorithm`

Copies nothing extra — `apply` leaves the in-place tile as the caller already
copied src → dst. Empty schema. CPU-only.

```rust
//! Pass-through (`pass_through`) — identity filter used as a template.

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;

pub struct PassThroughAlgorithm;

impl FilterAlgorithm for PassThroughAlgorithm {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("pass_through")
    }

    fn display_name(&self) -> &'static str {
        "Pass Through"
    }

    fn apply(
        &self,
        _tile: &mut PixelTile,
        _params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        Ok(())
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter)
    }

    fn param_schema(&self) -> &'static [ParamField] {
        &[]
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Stylize
    }
}
```

Register with `registry.register(Box::new(pass_through::PassThroughAlgorithm));`
and add `pass_through` to `ALGORITHM_ID_REGISTRY.txt` only if you actually ship it.
Do not leave this example registered in production builds.
