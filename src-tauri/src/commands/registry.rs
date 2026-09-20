//! Registry Tauri commands — schema and category listings for the UI.
//!
//! These commands read the process-wide built-in [`engine_project::algorithms::builtin_registry`].
//! They do not mutate document state.

use engine_project::algorithms::builtin_registry;
use engine_registry::{AlgorithmInfo, EffectCategory, ParamField};

fn parse_category(category: &str) -> Result<EffectCategory, String> {
    serde_json::from_value(serde_json::Value::String(category.to_string()))
        .map_err(|_| format!("unknown category: {category}"))
}

/// Parameter schema for a registered algorithm (Req 5.2).
#[tauri::command]
pub fn get_algorithm_schema(id: String) -> Result<Vec<ParamField>, String> {
    get_algorithm_schema_inner(&id)
}

pub(crate) fn get_algorithm_schema_inner(id: &str) -> Result<Vec<ParamField>, String> {
    builtin_registry()
        .get_by_str(id)
        .map(|algo| algo.param_schema().to_vec())
        .ok_or_else(|| format!("unknown algorithm: {id}"))
}

/// Algorithms in one [`EffectCategory`], for the effect chooser (Req 6.2).
#[tauri::command]
pub fn list_algorithms_for_category(category: String) -> Result<Vec<AlgorithmInfo>, String> {
    list_algorithms_for_category_inner(&category)
}

pub(crate) fn list_algorithms_for_category_inner(
    category: &str,
) -> Result<Vec<AlgorithmInfo>, String> {
    let cat = parse_category(category)?;
    Ok(builtin_registry().list_for_category(cat))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_registry::EffectCategory;

    #[test]
    fn schema_for_bayer_4x4_is_nonempty() {
        let schema = get_algorithm_schema_inner("bayer_4x4").expect("registered");
        assert!(!schema.is_empty());
        assert!(schema
            .iter()
            .any(|f| matches!(f, ParamField::Slider { key: "levels", .. })));
    }

    #[test]
    fn schema_unknown_id_errors() {
        let err = get_algorithm_schema_inner("algo_that_does_not_exist_v99").unwrap_err();
        assert!(err.contains("unknown algorithm"));
    }

    #[test]
    fn list_dithering_includes_bayer_and_floyd() {
        let list = list_algorithms_for_category_inner("dithering").expect("category");
        let ids: Vec<&str> = list.iter().map(|a| a.id).collect();
        assert!(ids.contains(&"bayer_4x4"));
        assert!(ids.contains(&"floyd_steinberg"));
        assert!(list.iter().all(|a| a.category == EffectCategory::Dithering));
    }

    #[test]
    fn list_unknown_category_errors() {
        let err = list_algorithms_for_category_inner("not_a_category").unwrap_err();
        assert!(err.contains("unknown category"));
    }
}
