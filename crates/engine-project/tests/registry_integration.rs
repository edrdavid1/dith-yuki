//! Integration tests for unknown-algorithm load/save (Req 8.5, 8.6).

use engine_project::filter::{FilterKind, FilterParams, PlaceholderParams};
use engine_project::serialize::{filter_from_file, filter_to_file, FilterInstanceFile};
use engine_project::types::FilterInstanceId;
use engine_project::BlendMode;

#[test]
fn unknown_algorithm_id_roundtrip() {
    let file = FilterInstanceFile {
        id: FilterInstanceId::new(),
        kind: FilterKind::Placeholder,
        params: serde_json::json!({"custom_key": 42}),
        enabled: true,
        opacity: 1.0,
        blend_mode: BlendMode::Normal,
        algorithm_id: Some("algo_that_does_not_exist_v99".into()),
        schema_version: Some(1),
    };

    let loaded = filter_from_file(&file, None);
    assert!(!loaded.enabled);
    assert_eq!(
        loaded.algorithm_id.as_deref(),
        Some("algo_that_does_not_exist_v99")
    );
    match &loaded.params {
        FilterParams::Placeholder(PlaceholderParams { label, raw_params }) => {
            assert_eq!(label, "algo_that_does_not_exist_v99");
            assert_eq!(
                raw_params.as_ref(),
                Some(&serde_json::json!({"custom_key": 42}))
            );
        }
        other => panic!("expected Placeholder, got {other:?}"),
    }

    let saved = filter_to_file(&loaded);
    assert_eq!(
        saved.algorithm_id.as_deref(),
        Some("algo_that_does_not_exist_v99")
    );
    assert_eq!(saved.params, serde_json::json!({"custom_key": 42}));
    assert!(!saved.enabled);

    let loaded_again = filter_from_file(&saved, None);
    assert!(!loaded_again.enabled);
    match &loaded_again.params {
        FilterParams::Placeholder(PlaceholderParams { label, raw_params }) => {
            assert_eq!(label, "algo_that_does_not_exist_v99");
            assert_eq!(
                raw_params.as_ref(),
                Some(&serde_json::json!({"custom_key": 42}))
            );
        }
        other => panic!("expected Placeholder, got {other:?}"),
    }
}
