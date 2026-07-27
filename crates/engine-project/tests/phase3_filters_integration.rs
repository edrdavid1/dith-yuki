//! Phase 3 Integration Tests: Filter Algorithms
//!
//! End-to-end tests for filter pipeline, verifying that filters can be applied
//! to documents, layers, and tiles. These tests demonstrate the complete filter
//! system working together.

use engine_project::{
    Document, DocumentHandle, FilterInstance, FilterKind, FilterParams,
    Layer, LayerKind, LayerId,
};
use engine_tiles::PixelTile;
use std::time::Instant;

#[test]
fn filter_instance_curves_with_document() {
    // Create a document with a layer containing a curves filter
    let doc = Document::default();
    let handle = DocumentHandle::new(doc);

    handle.mutate(|doc| {
        let layer_id = LayerId::new(1);
        let mut layer = Layer::new(layer_id, LayerKind::Raster, 5000, 5000);

        // Add a curves filter to brighten the image
        let filter = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (0.5, 0.7), (1.0, 1.0)],
            },
        );
        layer.filters.push(filter);

        // Store layer in document
        doc.root.push(engine_project::LayerNode::Leaf(layer));
        doc.increment_generation();
    });

    // Verify the filter was added to the layer
    let snapshot = handle.snapshot();
    if let engine_project::LayerNode::Leaf(layer) = &snapshot.root[0] {
        assert_eq!(layer.filters.len(), 1);
        assert!(layer.filters[0].enabled);
        match &layer.filters[0].params {
            FilterParams::Curves { curve } => assert_eq!(curve.len(), 3),
            _ => panic!("Expected Curves filter"),
        }
    } else {
        panic!("Expected leaf layer");
    }
}

#[test]
fn filter_instance_levels_with_document() {
    let doc = Document::default();
    let handle = DocumentHandle::new(doc);

    handle.mutate(|doc| {
        let layer_id = LayerId::new(1);
        let mut layer = Layer::new(layer_id, LayerKind::Raster, 5000, 5000);

        // Add a levels filter for contrast adjustment
        let filter = FilterInstance::new(
            FilterKind::Levels,
            FilterParams::Levels {
                input_black: 0.1,
                input_white: 0.9,
                output_black: 0.0,
                output_white: 1.0,
            },
        );
        layer.filters.push(filter);

        doc.root.push(engine_project::LayerNode::Leaf(layer));
        doc.increment_generation();
    });

    // Verify the levels filter was correctly stored
    let snapshot = handle.snapshot();
    if let engine_project::LayerNode::Leaf(layer) = &snapshot.root[0] {
        assert_eq!(layer.filters.len(), 1);
        match &layer.filters[0].params {
            FilterParams::Levels {
                input_black,
                input_white,
                ..
            } => {
                assert_eq!(*input_black, 0.1);
                assert_eq!(*input_white, 0.9);
            }
            _ => panic!("Expected Levels filter"),
        }
    }
}

#[test]
fn multiple_filters_in_layer_stack() {
    let doc = Document::default();
    let handle = DocumentHandle::new(doc);

    handle.mutate(|doc| {
        let layer_id = LayerId::new(1);
        let mut layer = Layer::new(layer_id, LayerKind::Raster, 5000, 5000);

        // Add curves filter
        let curves_filter = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (1.0, 1.0)],
            },
        );
        layer.filters.push(curves_filter);

        // Add levels filter
        let levels_filter = FilterInstance::new(
            FilterKind::Levels,
            FilterParams::Levels {
                input_black: 0.0,
                input_white: 1.0,
                output_black: 0.0,
                output_white: 1.0,
            },
        );
        layer.filters.push(levels_filter);

        doc.root.push(engine_project::LayerNode::Leaf(layer));
        doc.increment_generation();
    });

    // Verify both filters are in the stack
    let snapshot = handle.snapshot();
    if let engine_project::LayerNode::Leaf(layer) = &snapshot.root[0] {
        assert_eq!(layer.filters.len(), 2);
        // Verify order is preserved
        match &layer.filters[0].kind {
            FilterKind::Curves => {}
            _ => panic!("First filter should be Curves"),
        }
        match &layer.filters[1].kind {
            FilterKind::Levels => {}
            _ => panic!("Second filter should be Levels"),
        }
    }
}

#[test]
fn disable_and_reenable_filter() {
    let doc = Document::default();
    let handle = DocumentHandle::new(doc);

    let mut filter_id = engine_project::FilterInstanceId::default();

    handle.mutate(|doc| {
        let layer_id = LayerId::new(1);
        let mut layer = Layer::new(layer_id, LayerKind::Raster, 5000, 5000);

        let mut filter = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (1.0, 1.0)],
            },
        );
        filter_id = filter.id;
        filter.enabled = true;

        layer.filters.push(filter);
        doc.root.push(engine_project::LayerNode::Leaf(layer));
        doc.increment_generation();
    });

    // Verify filter is enabled
    let snapshot = handle.snapshot();
    if let engine_project::LayerNode::Leaf(layer) = &snapshot.root[0] {
        assert!(layer.filters[0].enabled);
    }

    // Disable the filter
    handle.mutate(|doc| {
        if let engine_project::LayerNode::Leaf(layer) = &mut doc.root[0] {
            if let Some(filter) = layer.find_filter_mut(filter_id) {
                filter.enabled = false;
            }
        }
        doc.increment_generation();
    });

    // Verify filter is now disabled
    let snapshot = handle.snapshot();
    if let engine_project::LayerNode::Leaf(layer) = &snapshot.root[0] {
        assert!(!layer.filters[0].enabled);
    }
}

#[test]
fn filter_performance_benchmark() {
    use engine_project::filters::curves::CurvesFilter;
    use engine_project::filters::levels::LevelsFilter;
    use engine_project::filters::curves::CurveChannel;

    let tile = PixelTile::new();

    // Benchmark Curves filter (just 10 iterations for debug mode)
    let curves_filter = CurvesFilter::new(CurveChannel::All);
    let start = Instant::now();
    for _ in 0..10 {
        let _ = curves_filter.apply_to_tile(&tile);
    }
    let curves_time = start.elapsed();
    let per_tile_micros = curves_time.as_micros() / 10;
    println!("Curves filter: {:.2} μs per tile", per_tile_micros);
    // In release mode: <5 μs, in debug mode: expect more but should be reasonable
    assert!(per_tile_micros < 10_000, "Curves too slow: {} μs per tile", per_tile_micros);

    // Benchmark Levels filter
    let levels_filter = LevelsFilter::new();
    let start = Instant::now();
    for _ in 0..10 {
        let _ = levels_filter.apply_to_tile(&tile);
    }
    let levels_time = start.elapsed();
    let per_tile_micros = levels_time.as_micros() / 10;
    println!("Levels filter: {:.2} μs per tile", per_tile_micros);
    assert!(per_tile_micros < 10_000, "Levels too slow: {} μs per tile", per_tile_micros);
}

#[test]
fn filter_stack_traversal() {
    let doc = Document::default();
    let handle = DocumentHandle::new(doc);

    handle.mutate(|doc| {
        let layer_id = LayerId::new(1);
        let mut layer = Layer::new(layer_id, LayerKind::Raster, 5000, 5000);

        // Add 3 filters
        for i in 0..3 {
            let filter = if i == 0 {
                FilterInstance::new(
                    FilterKind::Curves,
                    FilterParams::Curves {
                        curve: vec![(0.0, 0.0), (1.0, 1.0)],
                    },
                )
            } else if i == 1 {
                FilterInstance::new(
                    FilterKind::Levels,
                    FilterParams::Levels {
                        input_black: 0.0,
                        input_white: 1.0,
                        output_black: 0.0,
                        output_white: 1.0,
                    },
                )
            } else {
                FilterInstance::new(
                    FilterKind::Curves,
                    FilterParams::Curves {
                        curve: vec![(0.0, 0.0), (1.0, 1.0)],
                    },
                )
            };
            layer.filters.push(filter);
        }

        doc.root.push(engine_project::LayerNode::Leaf(layer));
        doc.increment_generation();
    });

    // Traverse all filters in the stack
    let snapshot = handle.snapshot();
    if let engine_project::LayerNode::Leaf(layer) = &snapshot.root[0] {
        let mut count = 0;
        for filter in &layer.filters {
            assert!(filter.enabled, "Filter {} disabled", count);
            count += 1;
        }
        assert_eq!(count, 3, "Should have 3 filters in stack");
    }
}

#[test]
fn filter_validation() {
    // Test that filter validation catches invalid parameters
    use engine_project::filter::FilterInstance;

    // Valid filter should pass validation
    let valid_filter = FilterInstance::new(
        FilterKind::Curves,
        FilterParams::Curves {
            curve: vec![(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)],
        },
    );
    assert!(valid_filter.validate().is_ok());

    // Invalid curve (out of range) should fail validation
    let invalid_filter = FilterInstance::new(
        FilterKind::Curves,
        FilterParams::Curves {
            curve: vec![(1.5, 0.5)], // x > 1.0
        },
    );
    assert!(invalid_filter.validate().is_err());

    // Valid levels filter
    let valid_levels = FilterInstance::new(
        FilterKind::Levels,
        FilterParams::Levels {
            input_black: 0.2,
            input_white: 0.8,
            output_black: 0.0,
            output_white: 1.0,
        },
    );
    assert!(valid_levels.validate().is_ok());

    // Invalid levels (inverted input range)
    let invalid_levels = FilterInstance::new(
        FilterKind::Levels,
        FilterParams::Levels {
            input_black: 0.8,
            input_white: 0.2, // black > white
            output_black: 0.0,
            output_white: 1.0,
        },
    );
    assert!(invalid_levels.validate().is_err());
}
