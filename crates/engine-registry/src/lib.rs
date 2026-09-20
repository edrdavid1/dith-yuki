//! Algorithm registry for the Dither engine.
//!
//! This crate provides the `FilterAlgorithm` trait, `AlgorithmRegistry`, `FilterContext`,
//! and associated schema types that allow new algorithms to be added without touching
//! any existing dispatch code.
//!
//! # Adding a new algorithm
//! See `docs/HOW_TO_ADD_ALGORITHM.md` for the step-by-step guide.

use serde::{Deserialize, Serialize};

pub use engine_tiles::PixelTile;

// ---------------------------------------------------------------------------
// FilterContext — location note
// ---------------------------------------------------------------------------
//
// `FilterContext<'a>` is defined in `engine-project::filters::context` (and
// re-exported as `engine_project::FilterContext`).  It cannot live here in
// `engine-registry` because it references concrete types from `engine-project`
// (`Document`, `ErrorResidualsStore`) while `engine-project` itself depends on
// this crate — placing it here would introduce a circular crate dependency.
//
// This is the fallback described in the design document:
//   "Alternatively, if adding a full new crate is premature, the trait and types
//    can live in `engine-project/src/registry/mod.rs` as a module rather than a
//    separate crate. The interface is identical; the crate boundary decision does
//    not affect the public API."
//
// All types that *can* be defined here (AlgorithmId, FilterError, ParamField,
// GpuEligibility, EffectCategory, AlgorithmInfo, AlgorithmRegistry,
// FilterAlgorithm) remain in this crate.  `FilterAlgorithm::apply` takes a
// `&engine_project::FilterContext<'_>` — callers in `engine-project` have that
// type in scope naturally.
//
// See: `crates/engine-project/src/filters/context.rs`
// ---------------------------------------------------------------------------

/// Error type returned by [`FilterAlgorithm::apply`].
///
/// Two variants cover the two failure modes an algorithm can encounter:
///
/// - `Params` — the JSON params value could not be deserialised into the
///   algorithm's expected parameter type.  Constructed automatically via the
///   [`From<serde_json::Error>`] implementation (use `?` inside `apply`).
///
/// - `Engine` — the underlying engine function returned an error (e.g.
///   palette lookup failure, invalid coords).  Because `EngineError` lives in
///   `engine-project`, which in turn depends on this crate, we cannot import
///   it here without creating a circular dependency.  Instead we store the
///   error as its `Display` string.  `engine-project` provides a
///   `From<EngineError> for FilterError` conversion that calls
///   `FilterError::from_engine_error` — callers inside `engine-project` can
///   therefore still use `?` after adding `.map_err(FilterError::from_engine)?`.
#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    /// Parameter deserialisation failed.
    #[error("params deserialisation failed: {0}")]
    Params(#[from] serde_json::Error),

    /// An underlying engine operation failed.
    ///
    /// The error is stored as a `String` (the `Display` representation of the
    /// originating error) to avoid a circular crate dependency.
    /// Use [`FilterError::from_engine`] to construct this variant from any
    /// value that implements `std::fmt::Display`.
    #[error("engine error: {0}")]
    Engine(String),
}

impl FilterError {
    /// Construct a [`FilterError::Engine`] from any displayable error value.
    ///
    /// Intended for use as `e.map_err(FilterError::from_engine)` inside
    /// `engine-project`, where the concrete `EngineError` type is available.
    pub fn from_engine(e: impl std::fmt::Display) -> Self {
        FilterError::Engine(e.to_string())
    }
}

/// Stable, immutable identity of an algorithm.
///
/// Written to disk in saved `.dyproj` / `.dyuki` files. Once assigned and shipped,
/// an `AlgorithmId` is **never renamed, deleted, or reused**. All IDs ever shipped
/// are tracked in `ALGORITHM_ID_REGISTRY.txt` at the repository root; CI fails if
/// any entry disappears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AlgorithmId(pub &'static str);

impl AlgorithmId {
    /// Create a new `AlgorithmId` from a `'static str`.
    ///
    /// The string must be stable across all versions — once used in a saved file,
    /// it cannot change.
    pub const fn new(s: &'static str) -> Self {
        Self(s)
    }

    /// Return the underlying string slice.
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for AlgorithmId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

// ---------------------------------------------------------------------------
// CpuCheckpointKind — mirror of engine_gpu's type
// ---------------------------------------------------------------------------
//
// Defined here rather than imported from `engine-gpu` because `engine-registry`
// sits below `engine-gpu` in the crate dependency graph (engine-registry is a
// leaf crate; engine-gpu depends on it indirectly through engine-project).
// `engine-project` maps between this type and `engine_gpu::CpuCheckpointKind`
// at the boundary where it compiles the GPU graph.

/// Reason why a filter algorithm must run on the CPU rather than the GPU.
///
/// Used in [`GpuEligibility::Cpu`]. Mirrors `engine_gpu::CpuCheckpointKind`;
/// the two enums are kept in sync manually — add a variant here and in
/// `engine-gpu/src/graph/types.rs` at the same time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CpuCheckpointKind {
    /// The algorithm uses error diffusion, which is inherently sequential.
    ErrorDiffusion,
    /// The algorithm operates at block granularity, not compatible with GPU tiling.
    BlockGranularity,
    /// The Bayer / ordered dither parameters are not GPU-eligible (e.g. custom angle).
    IneligibleDither,
    /// The adjust filter requested blur, which requires a separable pass.
    AdjustBlur,
    /// The filter has no GPU implementation.
    UnsupportedFilter,
    /// Full-stack CPU fallback (GPU context unavailable or disabled).
    FullStackFallback,
}

// ---------------------------------------------------------------------------
// GpuEligibility
// ---------------------------------------------------------------------------

/// Whether an algorithm can run on the GPU for a given set of parameters.
///
/// Returned by [`FilterAlgorithm::gpu_eligibility`]. The GPU graph compiler in
/// `engine-project/src/filters/gpu_graph.rs` maps [`GpuEligibility::Eligible`]
/// to a concrete `GraphLayerFilter` using its own closed `match` (Invariant 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuEligibility {
    /// The algorithm can run on GPU with the given parameters.
    /// `gpu_graph.rs` converts this to the appropriate `GraphLayerFilter`.
    Eligible,
    /// The algorithm must run on CPU; carries the reason for diagnostics.
    Cpu(CpuCheckpointKind),
}

// ---------------------------------------------------------------------------
// EffectCategory
// ---------------------------------------------------------------------------

/// Coarse UI grouping for algorithms.
///
/// Owned by the backend Registry (Invariant 5). The frontend does not maintain
/// a duplicate mapping for registered algorithms — it queries
/// `list_algorithms_for_category` instead.
///
/// Adding a new variant requires changes in two places:
/// 1. This enum (backend).
/// 2. The `EffectType` union in `frontend/src/features/effects/effects.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectCategory {
    Dithering,
    Glitch,
    ColorAdjust,
    Stylize,
    Palette,
}

// ---------------------------------------------------------------------------
// ParamField — schema-driven UI panel descriptor
// ---------------------------------------------------------------------------

/// A single parameter field descriptor used to auto-generate the settings panel.
///
/// Returned by [`FilterAlgorithm::param_schema`]. The frontend renders a
/// control for each field based on its variant without any algorithm-specific code.
///
/// Adding a new variant requires:
/// 1. Adding it here (backend).
/// 2. Adding a `case` in the frontend `AlgorithmSettingsPanel` switch.
/// Both changes are shared by all future algorithms that use the new control type.
///
/// # Serde note
/// `ParamField` is serialise-only — it is sent outbound to the frontend via
/// Tauri commands but never deserialised back. The `Dropdown::options` field
/// holds a `'static` slice that serde cannot reconstruct on the inbound path.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParamField {
    /// A continuous numeric slider.
    Slider {
        /// Parameter key used in the JSON params object.
        key: &'static str,
        /// Human-readable label shown in the UI.
        label: &'static str,
        /// Minimum value (inclusive).
        min: f32,
        /// Maximum value (inclusive).
        max: f32,
        /// Default value.
        default: f32,
        /// Optional step size. `None` means the slider is continuous.
        step: Option<f32>,
    },
    /// A boolean toggle.
    Checkbox {
        key: &'static str,
        label: &'static str,
        default: bool,
    },
    /// A single-choice dropdown.
    Dropdown {
        key: &'static str,
        label: &'static str,
        /// `(value, display_label)` pairs. Both are `'static` strings.
        options: &'static [(&'static str, &'static str)],
        /// The default selected value (must match one of `options.0`).
        default: &'static str,
    },
}

// ---------------------------------------------------------------------------
// AlgorithmInfo — lightweight descriptor for UI lists
// ---------------------------------------------------------------------------

/// Lightweight descriptor returned by [`AlgorithmRegistry::list_for_category`].
///
/// Serialised and sent to the frontend via the `list_algorithms_for_category`
/// Tauri command. Contains only the fields needed to populate an effect picker.
#[derive(Debug, Clone, Serialize)]
pub struct AlgorithmInfo {
    /// Stable string ID (same as `AlgorithmId::as_str()`).
    pub id: &'static str,
    /// Human-readable name. May change freely between versions.
    pub display_name: &'static str,
    /// UI category.
    pub category: EffectCategory,
    /// If `true`, the UI should show a deprecation warning.
    pub deprecated: bool,
}

// ---------------------------------------------------------------------------
// AlgorithmRegistry
// ---------------------------------------------------------------------------

/// Runtime map from [`AlgorithmId`] to a boxed [`FilterAlgorithm`].
///
/// Built once at application start via `builtin_algorithms::register_all()` and
/// kept as immutable shared state for the rest of the process (Req 1.5).
///
/// # Duplicate registration
/// Registering the same `AlgorithmId` twice panics in debug builds (via
/// `debug_assert`). In release builds the second registration silently replaces
/// the first — the `register_all_ids_unique` CI test ensures this never ships.
pub struct AlgorithmRegistry {
    algorithms: std::collections::HashMap<AlgorithmId, Box<dyn FilterAlgorithm>>,
}

impl AlgorithmRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            algorithms: std::collections::HashMap::new(),
        }
    }

    /// Register an algorithm.
    ///
    /// # Panics (debug only)
    /// Panics in debug builds if an algorithm with the same `AlgorithmId` has
    /// already been registered.
    pub fn register(&mut self, algo: Box<dyn FilterAlgorithm>) {
        let id = algo.id();
        debug_assert!(
            !self.algorithms.contains_key(&id),
            "duplicate AlgorithmId registration: {:?}",
            id
        );
        self.algorithms.insert(id, algo);
    }

    /// Look up an algorithm by its stable ID.
    ///
    /// Returns `None` if no algorithm with that ID is registered (e.g. the ID
    /// comes from a document written by a newer version of the app).
    pub fn get(&self, id: AlgorithmId) -> Option<&dyn FilterAlgorithm> {
        self.algorithms.get(&id).map(|b| b.as_ref())
    }

    /// Look up an algorithm by a runtime ID string (e.g. from a saved file).
    ///
    /// `AlgorithmId` is `'static`; file/UI IDs are owned `String`s. This walks
    /// the small built-in map and compares `as_str()`.
    pub fn get_by_str(&self, id: &str) -> Option<&dyn FilterAlgorithm> {
        self.algorithms
            .iter()
            .find(|(k, _)| k.as_str() == id)
            .map(|(_, algo)| algo.as_ref())
    }

    /// Iterate over all registered [`AlgorithmId`]s.
    ///
    /// Order is unspecified (HashMap iteration).
    pub fn all_ids(&self) -> impl Iterator<Item = AlgorithmId> + '_ {
        self.algorithms.keys().copied()
    }

    /// Return all algorithms belonging to `cat`, as lightweight [`AlgorithmInfo`] descriptors.
    ///
    /// Order is unspecified. The frontend sorts for display.
    pub fn list_for_category(&self, cat: EffectCategory) -> Vec<AlgorithmInfo> {
        self.algorithms
            .values()
            .filter(|a| a.category() == cat)
            .map(|a| AlgorithmInfo {
                id: a.id().as_str(),
                display_name: a.display_name(),
                category: a.category(),
                deprecated: false,
            })
            .collect()
    }
}

impl Default for AlgorithmRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// FilterCtx — opaque context trait
// ---------------------------------------------------------------------------
//
// `engine-registry` cannot import `engine_project::FilterContext` directly
// because `engine-project` depends on this crate (circular dependency).
// Instead, `FilterAlgorithm::apply` takes a `&dyn FilterCtx`, and
// `engine_project::FilterContext` implements `FilterCtx`.
//
// Algorithm implementations in `engine-project` recover `&FilterContext<'_>`
// via `FilterContext::from_ctx`. `Any` downcasting is not used because
// `FilterContext` borrows caches and is therefore not `'static`.

/// Opaque context trait implemented by `engine_project::FilterContext`.
///
/// Passed to [`FilterAlgorithm::apply`]. Algorithms that need typed access to
/// document-scoped caches recover it with `FilterContext::from_ctx`:
///
/// ```ignore
/// let ctx = FilterContext::from_ctx(ctx);
/// ```
///
/// The `Send + Sync` bound ensures `Box<dyn FilterAlgorithm>` can be stored in
/// shared state and dispatched from multiple threads.
pub trait FilterCtx: Send + Sync {
    /// Type-erased pointer to the concrete `engine_project::FilterContext`.
    ///
    /// `FilterContext` is not `'static`, so `std::any::Any` cannot be used.
    fn as_erased_context(&self) -> *const ();
}

// ---------------------------------------------------------------------------
// FilterAlgorithm — the central extensibility trait
// ---------------------------------------------------------------------------

/// The trait every registered filter algorithm implements.
///
/// # Implementing a new algorithm
///
/// See `docs/HOW_TO_ADD_ALGORITHM.md` for the full step-by-step guide. In brief:
///
/// 1. Create `crates/engine-project/src/algorithms/<id>.rs`.
/// 2. Implement this trait on a unit struct.
/// 3. Call `registry.register(Box::new(MyAlgo))` in `register_all()`.
/// 4. Add the `AlgorithmId` string to `ALGORITHM_ID_REGISTRY.txt`.
///
/// # Thread safety
///
/// The `Send + Sync` bound allows `Box<dyn FilterAlgorithm>` to be stored in
/// `AlgorithmRegistry` and shared across rayon worker threads.
pub trait FilterAlgorithm: Send + Sync {
    // ── Identity ──────────────────────────────────────────────────────────

    /// Stable identity written to disk.
    ///
    /// **Never rename this after the first release.** All IDs ever shipped are
    /// tracked in `ALGORITHM_ID_REGISTRY.txt`; CI fails if any entry disappears.
    fn id(&self) -> AlgorithmId;

    /// Human-readable name shown in the UI.
    ///
    /// May change freely between versions — it is never written to disk.
    fn display_name(&self) -> &'static str;

    // ── Core processing ───────────────────────────────────────────────────

    /// Apply the algorithm to `tile` in-place.
    ///
    /// # Parameters
    /// - `tile`   — the pixel tile to transform.
    /// - `params` — the algorithm's parameters as a JSON value. Deserialise
    ///              with `serde_json::from_value(params.clone())?` inside the
    ///              implementation.
    /// - `ctx`    — document-scoped dependencies. Recover
    ///              `engine_project::FilterContext<'_>` via `FilterContext::from_ctx`.
    ///
    /// # Errors
    /// Returns [`FilterError::Params`] if `params` cannot be deserialised, or
    /// [`FilterError::Engine`] if the underlying engine function fails.
    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError>;

    // ── GPU eligibility ───────────────────────────────────────────────────

    /// Whether this algorithm can run on the GPU for the given `params`.
    ///
    /// Returns [`GpuEligibility::Eligible`] if GPU dispatch is possible, or
    /// [`GpuEligibility::Cpu`] with a reason if not.
    ///
    /// The GPU graph compiler in `gpu_graph.rs` maps `Eligible` to a concrete
    /// `GraphLayerFilter` using its own closed `match` — the Registry only
    /// produces the signal, it does not compile GPU passes (Invariant 4).
    fn gpu_eligibility(&self, params: &serde_json::Value) -> GpuEligibility;

    // ── Schema-driven UI ──────────────────────────────────────────────────

    /// Parameter schema for automatic UI panel generation.
    ///
    /// Returns a static slice of [`ParamField`] descriptors. The frontend
    /// `AlgorithmSettingsPanel` renders one control per descriptor.
    ///
    /// Must return the same slice on every call (Property R-5).
    fn param_schema(&self) -> &'static [ParamField];

    /// Current version of this algorithm's parameter schema.
    ///
    /// Starts at `1`. Increment only on backwards-incompatible changes.
    /// Do **not** increment for additive changes handled by `serde` defaults.
    /// Stored in saved files so [`migrate_params`](Self::migrate_params) can
    /// be called on load.
    fn schema_version(&self) -> u32;

    // ── Tile ordering ─────────────────────────────────────────────────────

    /// Whether this algorithm requires row-major (full-row) tile ordering.
    ///
    /// Error-diffusion algorithms (Floyd-Steinberg, Atkinson, etc.) must process
    /// tiles in row order because they propagate residuals to adjacent tiles.
    /// All other algorithms return `false`.
    ///
    /// Default implementation returns `false`.
    fn requires_full_row(&self) -> bool {
        false
    }

    // ── Categorisation ────────────────────────────────────────────────────

    /// UI category for this algorithm.
    ///
    /// Drives `list_algorithms_for_category`. May change freely between versions
    /// — it is never persisted to disk (Req 6.5).
    fn category(&self) -> EffectCategory;

    // ── Format versioning ─────────────────────────────────────────────────

    /// Migrate `params` JSON in-place from `old_version` to the current schema.
    ///
    /// Called during document load when the saved `schema_version` differs from
    /// [`schema_version`](Self::schema_version).
    ///
    /// # Contract (Property R-4)
    /// Must be **pure** and **idempotent**: calling it twice with the same
    /// `old_version` must produce the same result as calling it once. Must never
    /// panic — leave `params` unchanged and log a warning on unexpected input.
    ///
    /// Default implementation is a no-op (algorithms that have never changed
    /// their schema require no implementation).
    fn migrate_params(&self, _old_version: u32, _params: &mut serde_json::Value) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn algorithm_id_new_and_as_str() {
        let id = AlgorithmId::new("bayer_4x4");
        assert_eq!(id.as_str(), "bayer_4x4");
    }

    #[test]
    fn algorithm_id_const_construction() {
        const ID: AlgorithmId = AlgorithmId::new("floyd_steinberg");
        assert_eq!(ID.as_str(), "floyd_steinberg");
    }

    #[test]
    fn algorithm_id_eq_and_hash() {
        use std::collections::HashSet;
        let a = AlgorithmId::new("bayer_4x4");
        let b = AlgorithmId::new("bayer_4x4");
        let c = AlgorithmId::new("bayer_2x2");
        assert_eq!(a, b);
        assert_ne!(a, c);
        let mut set = HashSet::new();
        set.insert(a);
        set.insert(b); // same id — should not grow the set
        assert_eq!(set.len(), 1);
        set.insert(c);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn algorithm_id_copy() {
        let id = AlgorithmId::new("wave");
        let id2 = id; // Copy — original still usable
        assert_eq!(id, id2);
    }

    #[test]
    fn algorithm_id_serde_roundtrip() {
        let id = AlgorithmId::new("palette_quantize");
        let json = serde_json::to_string(&id).expect("serialise");
        // Serialises as a plain JSON string.
        assert_eq!(json, r#""palette_quantize""#);
        let display = format!("{}", id);
        assert_eq!(display, "palette_quantize");
    }

    // ── Task 0.5 tests ────────────────────────────────────────────────────

    /// CpuCheckpointKind mirrors engine_gpu's enum — verify all variants
    /// round-trip through serde so they can be stored in diagnostics payloads.
    #[test]
    fn cpu_checkpoint_kind_serde_roundtrip() {
        let variants = [
            CpuCheckpointKind::ErrorDiffusion,
            CpuCheckpointKind::BlockGranularity,
            CpuCheckpointKind::IneligibleDither,
            CpuCheckpointKind::AdjustBlur,
            CpuCheckpointKind::UnsupportedFilter,
            CpuCheckpointKind::FullStackFallback,
        ];
        for v in variants {
            let json = serde_json::to_string(&v).expect("serialise");
            let back: CpuCheckpointKind = serde_json::from_str(&json).expect("deserialise");
            assert_eq!(v, back);
        }
    }

    /// GpuEligibility::Eligible and Cpu variants are distinct.
    #[test]
    fn gpu_eligibility_variants() {
        let e = GpuEligibility::Eligible;
        let c = GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter);
        assert_ne!(e, c);
        assert_eq!(e, GpuEligibility::Eligible);
        assert_eq!(c, GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter));
    }

    /// EffectCategory serialises as snake_case strings.
    #[test]
    fn effect_category_serde() {
        assert_eq!(
            serde_json::to_string(&EffectCategory::Dithering).unwrap(),
            r#""dithering""#
        );
        assert_eq!(
            serde_json::to_string(&EffectCategory::ColorAdjust).unwrap(),
            r#""color_adjust""#
        );
        let back: EffectCategory = serde_json::from_str(r#""glitch""#).unwrap();
        assert_eq!(back, EffectCategory::Glitch);
    }

    /// ParamField::Slider round-trips through serde, including the `type` tag.
    #[test]
    fn param_field_slider_serde_roundtrip() {
        let field = ParamField::Slider {
            key: "levels",
            label: "Levels",
            min: 2.0,
            max: 256.0,
            default: 4.0,
            step: Some(1.0),
        };
        let json = serde_json::to_string(&field).expect("serialise");
        assert!(
            json.contains(r#""type":"slider""#),
            "missing type tag: {json}"
        );
        assert!(json.contains(r#""key":"levels""#));
    }

    /// ParamField::Checkbox and Dropdown also carry the `type` discriminant.
    #[test]
    fn param_field_checkbox_and_dropdown_have_type_tag() {
        let cb = ParamField::Checkbox {
            key: "enabled",
            label: "Enabled",
            default: true,
        };
        let dd = ParamField::Dropdown {
            key: "mode",
            label: "Mode",
            options: &[("rgb", "RGB"), ("gray", "Grayscale")],
            default: "rgb",
        };
        let cb_json = serde_json::to_string(&cb).unwrap();
        let dd_json = serde_json::to_string(&dd).unwrap();
        assert!(cb_json.contains(r#""type":"checkbox""#), "{cb_json}");
        assert!(dd_json.contains(r#""type":"dropdown""#), "{dd_json}");
    }

    /// A minimal FilterAlgorithm implementation can be constructed and its
    /// default methods return the specified defaults.
    ///
    /// This test acts as a compile-time check that the trait is well-formed
    /// and that default implementations compile correctly.
    #[test]
    fn filter_algorithm_defaults() {
        struct NoopAlgo;

        impl FilterAlgorithm for NoopAlgo {
            fn id(&self) -> AlgorithmId {
                AlgorithmId::new("noop")
            }
            fn display_name(&self) -> &'static str {
                "No-op"
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

        // Safety: Send + Sync required for Box<dyn FilterAlgorithm>.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<NoopAlgo>();

        let algo = NoopAlgo;
        // Default: requires_full_row returns false.
        assert!(!algo.requires_full_row());
        // Default: migrate_params is a no-op (does not panic).
        let mut params = serde_json::json!({"key": "value"});
        algo.migrate_params(1, &mut params);
        assert_eq!(params, serde_json::json!({"key": "value"}));
        // Default schema is empty.
        assert!(algo.param_schema().is_empty());
        // schema_version is 1.
        assert_eq!(algo.schema_version(), 1);
    }

    /// Box<dyn FilterAlgorithm> can be stored and called — verifies the trait
    /// is object-safe and the Send + Sync bounds hold for heap allocation.
    #[test]
    fn filter_algorithm_is_object_safe() {
        struct PassThrough;

        impl FilterAlgorithm for PassThrough {
            fn id(&self) -> AlgorithmId {
                AlgorithmId::new("pass_through")
            }
            fn display_name(&self) -> &'static str {
                "Pass Through"
            }
            fn apply(
                &self,
                _t: &mut PixelTile,
                _p: &serde_json::Value,
                _c: &dyn FilterCtx,
            ) -> Result<(), FilterError> {
                Ok(())
            }
            fn gpu_eligibility(&self, _: &serde_json::Value) -> GpuEligibility {
                GpuEligibility::Eligible
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

        let boxed: Box<dyn FilterAlgorithm> = Box::new(PassThrough);
        assert_eq!(boxed.id().as_str(), "pass_through");
        assert_eq!(boxed.display_name(), "Pass Through");
        assert!(!boxed.requires_full_row());
    }

    // ── Task 0.6 tests ────────────────────────────────────────────────────

    // Minimal test algorithm factory — creates a zero-sized struct implementing
    // FilterAlgorithm with a given ID and category, suitable for registry tests.
    fn make_algo(id: &'static str, cat: EffectCategory) -> Box<dyn FilterAlgorithm> {
        struct Stub {
            id: &'static str,
            cat: EffectCategory,
        }
        impl FilterAlgorithm for Stub {
            fn id(&self) -> AlgorithmId {
                AlgorithmId::new(self.id)
            }
            fn display_name(&self) -> &'static str {
                self.id
            }
            fn apply(
                &self,
                _t: &mut PixelTile,
                _p: &serde_json::Value,
                _c: &dyn FilterCtx,
            ) -> Result<(), FilterError> {
                Ok(())
            }
            fn gpu_eligibility(&self, _: &serde_json::Value) -> GpuEligibility {
                GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter)
            }
            fn param_schema(&self) -> &'static [ParamField] {
                &[]
            }
            fn schema_version(&self) -> u32 {
                1
            }
            fn category(&self) -> EffectCategory {
                self.cat
            }
        }
        Box::new(Stub { id, cat })
    }

    /// Registering a single algorithm and looking it up by ID succeeds.
    #[test]
    fn registry_register_and_get() {
        let mut reg = AlgorithmRegistry::new();
        reg.register(make_algo("test_algo", EffectCategory::Dithering));

        let found = reg.get(AlgorithmId::new("test_algo"));
        assert!(found.is_some());
        assert_eq!(found.unwrap().id().as_str(), "test_algo");

        // Unknown ID returns None.
        assert!(reg.get(AlgorithmId::new("nonexistent")).is_none());
    }

    /// `all_ids()` returns every registered ID exactly once.
    #[test]
    fn registry_all_ids_complete() {
        let mut reg = AlgorithmRegistry::new();
        reg.register(make_algo("alpha", EffectCategory::Dithering));
        reg.register(make_algo("beta", EffectCategory::Glitch));
        reg.register(make_algo("gamma", EffectCategory::Dithering));

        let mut ids: Vec<&str> = reg.all_ids().map(|id| id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["alpha", "beta", "gamma"]);
    }

    /// `list_for_category` returns only algorithms in the requested category.
    #[test]
    fn registry_list_for_category() {
        let mut reg = AlgorithmRegistry::new();
        reg.register(make_algo("dither_a", EffectCategory::Dithering));
        reg.register(make_algo("dither_b", EffectCategory::Dithering));
        reg.register(make_algo("glitch_x", EffectCategory::Glitch));

        let dithering = reg.list_for_category(EffectCategory::Dithering);
        assert_eq!(dithering.len(), 2);
        assert!(dithering
            .iter()
            .all(|i| i.category == EffectCategory::Dithering));

        let glitch = reg.list_for_category(EffectCategory::Glitch);
        assert_eq!(glitch.len(), 1);
        assert_eq!(glitch[0].id, "glitch_x");

        // Category with no registrations returns empty vec.
        let palette = reg.list_for_category(EffectCategory::Palette);
        assert!(palette.is_empty());
    }

    /// Manually registering two algorithms with distinct IDs produces no
    /// duplicates — `all_ids()` has no repeats.
    ///
    /// Note: the full `register_all_ids_unique` test (which calls
    /// `engine_project::algorithms::register_all`) lives in
    /// `crates/engine-project/tests/registry_unit.rs` (task 0.7), because
    /// `register_all` is defined in `engine-project`.
    #[test]
    fn registry_manual_two_ids_unique() {
        let mut reg = AlgorithmRegistry::new();
        reg.register(make_algo("algo_one", EffectCategory::Stylize));
        reg.register(make_algo("algo_two", EffectCategory::ColorAdjust));

        let ids: Vec<AlgorithmId> = reg.all_ids().collect();
        // No duplicates.
        let unique: std::collections::HashSet<AlgorithmId> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len(), "duplicate IDs found");
        assert_eq!(ids.len(), 2);
    }

    /// `AlgorithmRegistry::default()` produces an empty registry, same as `new()`.
    #[test]
    fn registry_default_is_empty() {
        let reg = AlgorithmRegistry::default();
        assert_eq!(reg.all_ids().count(), 0);
    }

    /// `debug_assert` in register fires on duplicate ID in debug builds.
    /// In release builds the second registration replaces the first
    /// (no panic), which the CI `register_all_ids_unique` test prevents from
    /// shipping.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "duplicate AlgorithmId registration")]
    fn registry_duplicate_panics_in_debug() {
        let mut reg = AlgorithmRegistry::new();
        reg.register(make_algo("dup_id", EffectCategory::Glitch));
        reg.register(make_algo("dup_id", EffectCategory::Glitch)); // should panic
    }
}
