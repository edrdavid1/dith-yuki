//! Post / barrier / Pre classification for the Dithered-cache plan (`.local-doc/post.md`).
//!
//! P1: pure keys + `classify_change`. Does **not** change residual clearing or
//! invalidation behavior — callers may log these values until P2.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::filter::{
    filter_params_to_json, resolve_algorithm_id, DitherMode, FilterInstance, FilterParams,
};
use crate::types::FilterInstanceId;

/// Algorithm cost for barrier placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CostClass {
    /// ED (CPU/GPU family), ASCII, full-document (e.g. Riemersma). May be a barrier.
    Expensive,
    /// Pointwise grade, ordered dither (incl. GPU Bayer), JPEG glitch, etc.
    Cheap,
}

/// Where a stack edit sits relative to the expensive barrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeClass {
    /// Only filters strictly above the barrier (toward Processed / panel top).
    Post,
    /// Barrier identity, params, or presence changed.
    Barrier,
    /// Filters below the barrier (toward Raw), or no-barrier full stack.
    Pre,
}

/// Inputs outside filter params that still affect the Dithered prefix.
#[derive(Debug, Clone, Default)]
pub struct PrefixExternals {
    pub pattern_version: u32,
    pub palette_fingerprint: u64,
    pub pixel_size: u8,
    pub raw_version: u64,
}

impl Hash for PrefixExternals {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pattern_version.hash(state);
        self.palette_fingerprint.hash(state);
        self.pixel_size.hash(state);
        self.raw_version.hash(state);
    }
}

/// Build externals that affect the Dithered prefix for one layer.
///
/// - `palette_fingerprint`: hash of every document palette id + colors
/// - `pixel_size`: from the barrier dither filter (else 1)
/// - `raw_version`: layer `raw_rev`
/// - `pattern_version`: reserved (0 until threshold/custom pattern versioning)
pub fn prefix_externals_for_layer(
    filters: &[FilterInstance],
    palettes: &[engine_color::palette::Palette],
    raw_rev: u64,
) -> PrefixExternals {
    let mut hasher = DefaultHasher::new();
    for p in palettes {
        p.id.hash(&mut hasher);
        for c in &p.colors {
            c.r.to_bits().hash(&mut hasher);
            c.g.to_bits().hash(&mut hasher);
            c.b.to_bits().hash(&mut hasher);
        }
    }
    let pixel_size = barrier_index(filters)
        .and_then(|i| filters.get(i))
        .and_then(|f| match &f.params {
            FilterParams::DitherV2(p) => Some(p.pixel_size),
            FilterParams::Dither { .. } => Some(1),
            _ => None,
        })
        .unwrap_or(1);
    PrefixExternals {
        pattern_version: 0,
        palette_fingerprint: hasher.finish(),
        pixel_size,
        raw_version: raw_rev,
    }
}

/// Cost class for one filter instance (enabled or not — callers filter).
pub fn cost_class_for_instance(filter: &FilterInstance) -> CostClass {
    cost_class_for_params(&filter.params, filter.algorithm_id.as_deref())
}

pub fn cost_class_for_params(params: &FilterParams, algorithm_id: Option<&str>) -> CostClass {
    match params {
        FilterParams::Ascii(_) => CostClass::Expensive,
        FilterParams::DitherV2(p) => {
            if p.mode.is_error_diffusion()
                || p.mode.is_ed1d()
                || matches!(
                    p.mode,
                    crate::filter::DitherModeV2::Riemersma
                        | crate::filter::DitherModeV2::BlockedFs
                        | crate::filter::DitherModeV2::BlueNoise
                )
                || (p.wrap_padding && p.mode.diffusion_kernel().is_some())
            {
                CostClass::Expensive
            } else {
                CostClass::Cheap
            }
        }
        FilterParams::Dither { mode, .. } => match mode {
            DitherMode::ErrorDiffusion { .. } => CostClass::Expensive,
            _ => CostClass::Cheap,
        },
        _ => {
            // Registry full-document algorithms without Ascii params (defensive).
            if algorithm_id == Some("riemersma") || algorithm_id == Some("ascii") {
                CostClass::Expensive
            } else {
                CostClass::Cheap
            }
        }
    }
}

/// Index of the topmost enabled Expensive filter in panel order (`filters[0]` = top /
/// final look), or `None` if the stack has no expensive stage.
pub fn barrier_index(filters: &[FilterInstance]) -> Option<usize> {
    filters
        .iter()
        .enumerate()
        .filter(|(_, f)| f.enabled && cost_class_for_instance(f) == CostClass::Expensive)
        .map(|(i, _)| i)
        .min()
}

fn hash_filter_body(filter: &FilterInstance, hasher: &mut DefaultHasher) {
    filter.id.as_u128().hash(hasher);
    format!("{:?}", filter.kind).hash(hasher);
    resolve_algorithm_id(filter).hash(hasher);
    let json = filter_params_to_json(&filter.params).unwrap_or(serde_json::Value::Null);
    json.to_string().hash(hasher);
    filter.enabled.hash(hasher);
    filter.opacity.to_bits().hash(hasher);
    format!("{:?}", filter.blend_mode).hash(hasher);
}

/// Fingerprint of one filter for equality / classify.
pub fn filter_fingerprint(filter: &FilterInstance) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_filter_body(filter, &mut hasher);
    hasher.finish()
}

/// Prefix hash through the barrier (barrier + everything below toward Raw).
///
/// Panel order: `filters[barrier..]` . With no barrier, hashes all enabled filters
/// (metrics only — no `Dithered` cache in that case).
pub fn prefix_hash_through_barrier(filters: &[FilterInstance], externals: &PrefixExternals) -> u64 {
    let mut hasher = DefaultHasher::new();
    externals.hash(&mut hasher);
    let start = barrier_index(filters).unwrap_or(0);
    for filter in filters[start..].iter().filter(|f| f.enabled) {
        if crate::filters::neutral::filter_is_neutral_for_hash(filter) {
            continue;
        }
        hash_filter_body(filter, &mut hasher);
    }
    // Distinct from “empty enabled prefix”.
    b"post_barrier_v1".hash(&mut hasher);
    hasher.finish()
}

/// Full enabled-stack hash (same fields as prefix body, entire list).
pub fn stack_hash(filters: &[FilterInstance], externals: &PrefixExternals) -> u64 {
    let mut hasher = DefaultHasher::new();
    externals.hash(&mut hasher);
    for filter in filters.iter().filter(|f| f.enabled) {
        if crate::filters::neutral::filter_is_neutral_for_hash(filter) {
            continue;
        }
        hash_filter_body(filter, &mut hasher);
    }
    b"post_stack_v1".hash(&mut hasher);
    hasher.finish()
}

fn fingerprints(filters: &[FilterInstance]) -> Vec<Option<u64>> {
    filters
        .iter()
        .map(|f| {
            if f.enabled {
                Some(filter_fingerprint(f))
            } else {
                // Disabled still occupies an index; fingerprint includes enabled=false.
                Some(filter_fingerprint(f))
            }
        })
        .collect()
}

/// Enabled filters that actually change pixels (exclude disabled + neutral params).
fn material_filters(filters: &[FilterInstance]) -> Vec<&FilterInstance> {
    filters
        .iter()
        .filter(|f| f.enabled && !crate::filters::neutral::filter_is_neutral_for_apply(f))
        .collect()
}

fn material_fingerprints(filters: &[FilterInstance]) -> Vec<u64> {
    material_filters(filters)
        .into_iter()
        .map(filter_fingerprint)
        .collect()
}

/// True when only neutrals/disabled differed (Task 2: bump neither post nor dither gen).
pub fn material_stack_unchanged(old: &[FilterInstance], new: &[FilterInstance]) -> bool {
    material_fingerprints(old) == material_fingerprints(new)
}

/// Which generation Task 2 should bump for a stack edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackEditBump {
    /// Material fingerprints unchanged (neutral↔neutral / noop).
    None,
    /// Post-only material change — `post_gen` only.
    Post,
    /// Barrier / Pre / order — `dither_gen` + `document_gen`.
    Dither,
}

/// Decide Task 2 gen bump from `classify_change`, with neutral→neutral → [`StackEditBump::None`].
pub fn stack_edit_bump(old: &[FilterInstance], new: &[FilterInstance]) -> StackEditBump {
    if material_stack_unchanged(old, new) {
        return StackEditBump::None;
    }
    match classify_change(old, new) {
        ChangeClass::Post => StackEditBump::Post,
        ChangeClass::Barrier | ChangeClass::Pre => StackEditBump::Dither,
    }
}

/// Classify a stack edit. Pure; used by P0 logs and (later) P2 invalidation.
pub fn classify_change(old: &[FilterInstance], new: &[FilterInstance]) -> ChangeClass {
    let b_old = barrier_index(old);
    let b_new = barrier_index(new);

    // Adding/removing only enabled-neutral filters must not force ED (prefix
    // already ignores them). Barrier *index* may shift when neutrals are
    // inserted above the expensive filter — still Post (no ED).
    if material_fingerprints(old) == material_fingerprints(new)
        && (fingerprints(old) != fingerprints(new) || old.len() != new.len())
    {
        return ChangeClass::Post;
    }

    if old.len() != new.len() {
        // Add/remove of a material filter: if both sides lack a barrier and only
        // Cheap filters exist, treat as Pre. If barrier set changes, Barrier.
        return match (b_old, b_new) {
            (None, None) => ChangeClass::Pre,
            _ => ChangeClass::Barrier,
        };
    }

    let fp_old = fingerprints(old);
    let fp_new = fingerprints(new);

    if b_old != b_new {
        return ChangeClass::Barrier;
    }

    let Some(b) = b_old else {
        // No expensive filter: any diff is a full-stack (Pre-like) change.
        return if fp_old == fp_new {
            ChangeClass::Post // identical — caller shouldn't ask; default Post is harmless
        } else {
            ChangeClass::Pre
        };
    };

    // Barrier filter identity at index b must match.
    if fp_old.get(b) != fp_new.get(b) {
        return ChangeClass::Barrier;
    }

    let mut pre = false;
    let mut post = false;
    for i in 0..old.len() {
        if fp_old[i] == fp_new[i] {
            continue;
        }
        if i > b {
            pre = true;
        } else if i < b {
            post = true;
        } else {
            // i == b already handled
            return ChangeClass::Barrier;
        }
    }

    if pre {
        ChangeClass::Pre
    } else if post {
        ChangeClass::Post
    } else {
        // No fingerprint diff (e.g. only externals) — treat as Barrier so callers
        // stay conservative when stacks look equal.
        ChangeClass::Barrier
    }
}

/// Ensure every filter has a non-nil id (generate if somehow default/zero).
/// Normal loads already persist UUIDs; this is a safety net after migrate.
pub fn ensure_filter_ids(filters: &mut [FilterInstance]) {
    for f in filters.iter_mut() {
        if f.id.as_u128() == 0 {
            f.id = FilterInstanceId::new();
        }
    }
}

/// Enabled filters in **apply order** (panel bottom → top), split at the barrier.
///
/// - `through`: Raw → … → barrier (inclusive). Identity of `Dithered`.
/// - `post`: filters strictly above the barrier (toward Processed).
///
/// Returns `None` when there is no expensive barrier (no `Dithered` cache).
pub fn split_enabled_at_barrier(
    filters: &[FilterInstance],
) -> Option<(Vec<&FilterInstance>, Vec<&FilterInstance>)> {
    let b = barrier_index(filters)?;
    let through: Vec<&FilterInstance> = filters[b..].iter().rev().filter(|f| f.enabled).collect();
    let post: Vec<&FilterInstance> = filters[..b].iter().rev().filter(|f| f.enabled).collect();
    Some((through, post))
}

/// Panel indices strictly above the barrier (Post region), or empty if none.
pub fn post_region_indices(filters: &[FilterInstance]) -> Vec<usize> {
    match barrier_index(filters) {
        Some(b) => (0..b).collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{
        DitherModeV2, DitherParamsV2, FilterKind, FilterParams, PlaceholderParams,
    };
    use crate::types::FilterInstanceId;

    fn adjust() -> FilterInstance {
        FilterInstance::new(
            FilterKind::Adjust,
            FilterParams::Adjust {
                contrast: 0.0,
                brightness: 0.0,
                saturation: 0.0,
                blur: 0.0,
                sharpness: 0.0,
                noise: 0.0,
                exposure: 0.0,
                shadows: 0.0,
                highlights: 0.0,
                temperature: 0.0,
                tint: 0.0,
                vibrance: 0.0,
                hue: 0.0,
                clarity: 0.0,
            },
        )
    }

    fn curves() -> FilterInstance {
        FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (1.0, 1.0)],
                channel: crate::filters::curves::CurveChannel::All,
            },
        )
    }

    fn floyd() -> FilterInstance {
        let mut p = DitherParamsV2::default();
        p.mode = DitherModeV2::FloydSteinberg;
        FilterInstance::new(FilterKind::Dither, FilterParams::DitherV2(p))
    }

    fn bayer() -> FilterInstance {
        let mut p = DitherParamsV2::default();
        p.mode = DitherModeV2::Bayer4x4;
        FilterInstance::new(FilterKind::Dither, FilterParams::DitherV2(p))
    }

    fn ascii() -> FilterInstance {
        FilterInstance::new(
            FilterKind::Ascii,
            FilterParams::Ascii(crate::filter::AsciiParams::default()),
        )
    }

    #[test]
    fn cost_class_ed_expensive_ordered_cheap() {
        assert_eq!(cost_class_for_instance(&floyd()), CostClass::Expensive);
        assert_eq!(cost_class_for_instance(&bayer()), CostClass::Cheap);
        assert_eq!(cost_class_for_instance(&adjust()), CostClass::Cheap);
        assert_eq!(cost_class_for_instance(&ascii()), CostClass::Expensive);
    }

    #[test]
    fn barrier_is_topmost_expensive() {
        // Panel top = index 0
        let stack = vec![curves(), floyd(), adjust()];
        assert_eq!(barrier_index(&stack), Some(1));
        let only_cheap = vec![curves(), bayer(), adjust()];
        assert_eq!(barrier_index(&only_cheap), None);
        let two_ed = vec![floyd(), adjust(), ascii()];
        // topmost expensive = floyd at 0
        assert_eq!(barrier_index(&two_ed), Some(0));
    }

    #[test]
    fn prefix_stable_when_neutral_pre_adjust_present() {
        let ext = PrefixExternals::default();
        let c = curves();
        let d = floyd();
        let n = adjust();
        let h_without = prefix_hash_through_barrier(&[c.clone(), d.clone()], &ext);
        let h_with = prefix_hash_through_barrier(&[c, d, n], &ext);
        assert_eq!(
            h_with, h_without,
            "neutral Pre Adjust must not affect prefix hash"
        );
    }

    #[test]
    fn prefix_stable_when_post_changes() {
        let mut stack = vec![curves(), floyd(), adjust()];
        let ext = PrefixExternals::default();
        let h0 = prefix_hash_through_barrier(&stack, &ext);
        if let FilterParams::Curves { curve, .. } = &mut stack[0].params {
            curve.push((0.5, 0.6));
        }
        let h1 = prefix_hash_through_barrier(&stack, &ext);
        assert_eq!(h0, h1, "Post-only change must not move prefix hash");
    }

    #[test]
    fn prefix_changes_on_pre_or_barrier() {
        let mut stack = vec![curves(), floyd(), adjust()];
        let ext = PrefixExternals::default();
        let h0 = prefix_hash_through_barrier(&stack, &ext);
        if let FilterParams::Adjust { brightness, .. } = &mut stack[2].params {
            *brightness = 0.2;
        }
        assert_ne!(h0, prefix_hash_through_barrier(&stack, &ext));

        let mut stack2 = vec![curves(), floyd(), adjust()];
        if let FilterParams::DitherV2(p) = &mut stack2[1].params {
            p.levels = 8;
        }
        assert_ne!(h0, prefix_hash_through_barrier(&stack2, &ext));
    }

    #[test]
    fn floyd_invert_scrub_is_post_only() {
        // Panel: Invert above Floyd → Post scrub must not touch ED / prefix.
        let invert = FilterInstance::new(FilterKind::Invert, FilterParams::Invert {});
        let base = vec![invert.clone(), floyd()];
        let mut scrubbed = base.clone();
        // Replacing Invert with a material Threshold still stays Post (above barrier).
        scrubbed[0] = FilterInstance::new(
            FilterKind::Threshold,
            FilterParams::Threshold { threshold: 0.4 },
        );
        assert_eq!(classify_change(&base, &scrubbed), ChangeClass::Post);
        let ext = PrefixExternals::default();
        assert_eq!(
            prefix_hash_through_barrier(&base, &ext),
            prefix_hash_through_barrier(&scrubbed, &ext),
            "Post Invert/Threshold scrub must keep dither prefix hash"
        );
        assert_eq!(
            stack_edit_bump(&base, &scrubbed),
            StackEditBump::Post,
            "Invert scrub above Floyd → post_gen only"
        );
    }

    #[test]
    fn classify_post_pre_barrier() {
        let base = vec![curves(), floyd(), adjust()];

        let mut post = base.clone();
        if let FilterParams::Curves { curve, .. } = &mut post[0].params {
            curve.clear();
            curve.extend([(0.0, 0.0), (0.5, 0.4), (1.0, 1.0)]);
        }
        assert_eq!(classify_change(&base, &post), ChangeClass::Post);

        let mut pre = base.clone();
        if let FilterParams::Adjust { contrast, .. } = &mut pre[2].params {
            *contrast = 0.3;
        }
        assert_eq!(classify_change(&base, &pre), ChangeClass::Pre);

        let mut barrier = base.clone();
        if let FilterParams::DitherV2(p) = &mut barrier[1].params {
            p.pixel_size = 2;
        }
        assert_eq!(classify_change(&base, &barrier), ChangeClass::Barrier);
    }

    #[test]
    fn classify_reorder_across_barrier() {
        // Identity Curves is neutral — material fingerprints stay equal when it
        // crosses the barrier, so classify stays Post (no ED). A material Curves
        // change must still force Barrier.
        let old_neutral = vec![curves(), floyd(), adjust()];
        let new_neutral = vec![
            old_neutral[1].clone(),
            old_neutral[0].clone(),
            old_neutral[2].clone(),
        ];
        assert_eq!(
            classify_change(&old_neutral, &new_neutral),
            ChangeClass::Post,
            "neutral reorder across barrier must not force ED"
        );

        let mut material = curves();
        if let FilterParams::Curves { curve, .. } = &mut material.params {
            *curve = vec![(0.0, 0.0), (0.5, 0.4), (1.0, 1.0)];
        }
        let old = vec![material, floyd(), adjust()];
        // Move material Curves under dither: [floyd, curves, adjust]
        let new = vec![old[1].clone(), old[0].clone(), old[2].clone()];
        assert_eq!(classify_change(&old, &new), ChangeClass::Barrier);
    }

    #[test]
    fn classify_enable_toggle() {
        let mut old = vec![curves(), floyd(), adjust()];
        let mut new = old.clone();
        new[0].enabled = false;
        assert_eq!(classify_change(&old, &new), ChangeClass::Post);
        old[1].enabled = false;
        new = old.clone();
        new[1].enabled = true;
        // enabling floyd when it was off changes barrier presence
        let disabled_ed = vec![
            curves(),
            {
                let mut f = floyd();
                f.enabled = false;
                f
            },
            adjust(),
        ];
        let enabled_ed = vec![curves(), floyd(), adjust()];
        assert_eq!(
            classify_change(&disabled_ed, &enabled_ed),
            ChangeClass::Barrier
        );
    }

    #[test]
    fn classify_add_second_expensive() {
        let old = vec![curves(), floyd(), adjust()];
        let new = vec![curves(), floyd(), ascii(), adjust()];
        assert_eq!(classify_change(&old, &new), ChangeClass::Barrier);
    }

    #[test]
    fn prefix_externals_include_pixel_size_and_palette() {
        use engine_color::palette::{LinearColor, Palette};

        let stack = vec![curves(), floyd(), adjust()];
        let palettes = vec![Palette {
            id: 1,
            name: "t".into(),
            colors: vec![LinearColor {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            }],
            revision: 0,
        }];
        let ext = prefix_externals_for_layer(&stack, &palettes, 7);
        assert_eq!(ext.raw_version, 7);
        assert_eq!(ext.pixel_size, 1); // floyd default
        assert_ne!(ext.palette_fingerprint, 0);
        let ext2 = prefix_externals_for_layer(&stack, &palettes, 8);
        assert_ne!(
            prefix_hash_through_barrier(&stack, &ext),
            prefix_hash_through_barrier(&stack, &ext2)
        );
    }

    #[test]
    fn split_enabled_through_then_post() {
        let stack = vec![curves(), floyd(), adjust()];
        let (through, post) = split_enabled_at_barrier(&stack).unwrap();
        assert_eq!(through.len(), 2); // adjust then floyd
        assert_eq!(through[0].kind, FilterKind::Adjust);
        assert_eq!(through[1].kind, FilterKind::Dither);
        assert_eq!(post.len(), 1);
        assert_eq!(post[0].kind, FilterKind::Curves);
        assert!(split_enabled_at_barrier(&[curves(), bayer(), adjust()]).is_none());
    }

    #[test]
    fn ensure_filter_ids_fills_nil() {
        let mut f = FilterInstance::new(
            FilterKind::Placeholder,
            FilterParams::Placeholder(PlaceholderParams {
                label: "x".into(),
                raw_params: None,
            }),
        );
        f.id = FilterInstanceId(uuid::Uuid::nil());
        ensure_filter_ids(std::slice::from_mut(&mut f));
        assert_ne!(f.id.as_u128(), 0);
    }

    #[test]
    fn stack_edit_bump_post_pre_and_neutral() {
        let base = vec![curves(), floyd(), adjust()];
        let mut post = base.clone();
        if let FilterParams::Curves { ref mut curve, .. } = post[0].params {
            *curve = vec![(0.0, 0.0), (0.5, 0.6), (1.0, 1.0)];
        }
        assert_eq!(stack_edit_bump(&base, &post), StackEditBump::Post);

        let mut pre = base.clone();
        if let FilterParams::Adjust {
            ref mut brightness, ..
        } = pre[2].params
        {
            *brightness = 0.2;
        }
        assert_eq!(stack_edit_bump(&base, &pre), StackEditBump::Dither);

        // Neutral Adjust → still-neutral Adjust (opacity only, still identity).
        let c = curves();
        let d = floyd();
        let mut n0 = adjust();
        n0.opacity = 1.0;
        let mut n1 = n0.clone();
        n1.opacity = 0.0; // opacity 0 is also neutral
        let with0 = vec![c.clone(), d.clone(), n0];
        let with1 = vec![c, d, n1];
        assert!(material_stack_unchanged(&with0, &with1));
        assert_eq!(stack_edit_bump(&with0, &with1), StackEditBump::None);
    }
}
