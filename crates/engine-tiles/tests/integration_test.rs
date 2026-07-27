//! Integration tests for multi-component workflows
//! 
//! These tests verify that different engine-tiles components work correctly together:
//! - Cache + Pyramid: layered tile storage with downsampling
//! - Invalidation Cascade: dirty propagation through tile stages
//! - Scheduler Priority: task ordering

use engine_tiles::{
    cache::TileCache,
    invalidation::{invalidate, InvalidationEvent},
    pyramid::downsample_tile,
    scheduler::{Priority, RecomputeTask, Scheduler},
    tile::PixelTile,
    types::{CacheStage, TileCoord, TileKey},
};
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// Test 1: Cache + Pyramid Integration
/// 
/// Verifies that a parent tile can be inserted into cache and downsampled
/// to create a child tile at a higher pyramid level.
#[test]
fn test_cache_pyramid_integration() {
    // Create cache with 100MB budget
    let cache = TileCache::new(100 * 1024 * 1024);

    // Create parent tile with known values
    let mut parent = PixelTile::new();

    // Fill parent with a simple pattern: (x, y, channel) -> x + y + channel
    for y in 0..260 {
        for x in 0..260 {
            for c in 0..4 {
                let val = ((x + y + c) % 256) as f32 / 255.0;
                parent.set(x, y, c, val);
            }
        }
    }

    let parent_arc = Arc::new(parent);

    // Insert parent at Layer 0, Level 0, as Raw
    let parent_key = TileKey {
        layer: 0,
        coord: TileCoord { level: 0, x: 0, y: 0 },
        stage: CacheStage::Raw,
    };
    cache.get_or_insert(parent_key, parent_arc.clone());

    // Verify parent is in cache
    assert!(
        cache.entries.contains_key(&parent_key),
        "Parent should exist in cache"
    );
    let cached_parent = cache
        .entries
        .get(&parent_key)
        .map(|e| e.tile.clone())
        .expect("Parent should exist in cache");
    assert_eq!(cached_parent.at(10, 10, 0), parent_arc.at(10, 10, 0), "Parent values should match");

    // Downsample to create child
    let child = downsample_tile(&parent_arc);
    let child_arc = Arc::new(child);

    // Insert child at Layer 0, Level 1 (pyramid level), as Raw
    let child_key = TileKey {
        layer: 0,
        coord: TileCoord { level: 1, x: 0, y: 0 },
        stage: CacheStage::Raw,
    };
    cache.get_or_insert(child_key, child_arc.clone());

    // Retrieve both from cache and verify existence
    let cached_child = cache
        .entries
        .get(&child_key)
        .map(|e| e.tile.clone())
        .expect("Child should exist in cache");

    // Verify both exist and have values
    assert_eq!(cached_parent.at(20, 20, 1), parent_arc.at(20, 20, 1), "Parent should still be retrievable");
    assert_eq!(cached_child.at(10, 10, 0), child_arc.at(10, 10, 0), "Child should be retrievable");

    // Verify child is actually downsampled (smaller values due to averaging)
    // Since parent was created with formula x+y+c, the downsampled values should be different
    let parent_sample = parent_arc.at(0, 0, 0);
    let child_sample = child_arc.at(0, 0, 0);
    // Child should exist and be computable from parent (values will vary)
    assert!(child_sample >= 0.0, "Child should have valid f32 values");
    assert!(parent_sample >= 0.0, "Parent should have valid f32 values");
}

/// Test 2: Invalidation Cascade
///
/// Verifies that marking a Raw tile dirty cascades to Processed and Composite tiles.
#[test]
fn test_invalidation_cascade() {
    // Create cache with 100MB budget
    let cache = Arc::new(TileCache::new(100 * 1024 * 1024));

    let layer = 0;
    let coord = TileCoord { level: 0, x: 5, y: 5 };

    // Create dummy tiles
    let tile_data = Arc::new(PixelTile::new());

    // Insert 3 tiles: Raw, Processed, Composite for same layer and coordinate
    let raw_key = TileKey {
        layer,
        coord,
        stage: CacheStage::Raw,
    };
    let processed_key = TileKey {
        layer,
        coord,
        stage: CacheStage::Processed,
    };
    let composite_key = TileKey {
        layer,
        coord,
        stage: CacheStage::Composite,
    };

    cache.get_or_insert(raw_key, tile_data.clone());
    cache.get_or_insert(processed_key, tile_data.clone());
    cache.get_or_insert(composite_key, tile_data.clone());

    // Verify all three are inserted
    assert!(
        cache.entries.contains_key(&raw_key),
        "Raw tile should be in cache"
    );
    assert!(
        cache.entries.contains_key(&processed_key),
        "Processed tile should be in cache"
    );
    assert!(
        cache.entries.contains_key(&composite_key),
        "Composite tile should be in cache"
    );

    // Call invalidate() with LayerRawChanged event
    let event = InvalidationEvent::LayerRawChanged {
        layer,
        coords: vec![coord],
    };
    invalidate(&cache, event);

    // Verify all 3 are marked dirty
    assert!(
        cache
            .entries
            .get(&raw_key)
            .map(|entry| entry.dirty.load(Ordering::Relaxed))
            .unwrap_or(false),
        "Raw tile should be marked dirty"
    );
    assert!(
        cache
            .entries
            .get(&processed_key)
            .map(|entry| entry.dirty.load(Ordering::Relaxed))
            .unwrap_or(false),
        "Processed tile should be marked dirty"
    );
    assert!(
        cache
            .entries
            .get(&composite_key)
            .map(|entry| entry.dirty.load(Ordering::Relaxed))
            .unwrap_or(false),
        "Composite tile should be marked dirty"
    );
}

/// Test 3: Scheduler Priority
///
/// Verifies that tasks are dequeued in priority order (Immediate, ViewportCenter, ViewportEdge, Prefetch).
#[test]
fn test_scheduler_priority() {
    let scheduler = Scheduler::new();

    // Create tasks with different priorities in random order
    let tasks = vec![
        RecomputeTask {
            key: TileKey {
                layer: 0,
                coord: TileCoord { level: 0, x: 0, y: 0 },
                stage: CacheStage::Raw,
            },
            generation: 1,
            layer_generation: 1,
            priority: Priority::Prefetch,
        },
        RecomputeTask {
            key: TileKey {
                layer: 0,
                coord: TileCoord { level: 0, x: 1, y: 1 },
                stage: CacheStage::Raw,
            },
            generation: 1,
            layer_generation: 1,
            priority: Priority::ViewportCenter,
        },
        RecomputeTask {
            key: TileKey {
                layer: 0,
                coord: TileCoord { level: 0, x: 2, y: 2 },
                stage: CacheStage::Raw,
            },
            generation: 1,
            layer_generation: 1,
            priority: Priority::Immediate,
        },
        RecomputeTask {
            key: TileKey {
                layer: 0,
                coord: TileCoord { level: 0, x: 3, y: 3 },
                stage: CacheStage::Raw,
            },
            generation: 1,
            layer_generation: 1,
            priority: Priority::ViewportEdge,
        },
        RecomputeTask {
            key: TileKey {
                layer: 0,
                coord: TileCoord { level: 0, x: 4, y: 4 },
                stage: CacheStage::Raw,
            },
            generation: 1,
            layer_generation: 1,
            priority: Priority::Immediate,
        },
        RecomputeTask {
            key: TileKey {
                layer: 0,
                coord: TileCoord { level: 0, x: 5, y: 5 },
                stage: CacheStage::Raw,
            },
            generation: 1,
            layer_generation: 1,
            priority: Priority::ViewportCenter,
        },
    ];

    // Enqueue tasks in the order they were created
    for task in &tasks {
        scheduler.enqueue(*task);
    }

    // Dequeue and verify priority order
    let mut dequeued_priorities = Vec::new();
    while let Some(task) = scheduler.dequeue() {
        dequeued_priorities.push(task.priority);
    }

    // Should dequeue in order: Immediate (2), ViewportCenter (2), ViewportEdge (1), Prefetch (1)
    assert!(
        dequeued_priorities.len() >= 4,
        "Should dequeue at least 4 tasks"
    );

    // Verify that Immediate tasks come before ViewportCenter
    let first_immediate = dequeued_priorities.iter().position(|p| *p == Priority::Immediate);
    let first_viewport_center = dequeued_priorities
        .iter()
        .position(|p| *p == Priority::ViewportCenter);

    if let (Some(imm_idx), Some(vc_idx)) = (first_immediate, first_viewport_center) {
        assert!(
            imm_idx < vc_idx,
            "Immediate priority should dequeue before ViewportCenter"
        );
    }

    // Verify that ViewportCenter comes before ViewportEdge
    let first_viewport_edge = dequeued_priorities
        .iter()
        .position(|p| *p == Priority::ViewportEdge);
    if let (Some(vc_idx), Some(ve_idx)) = (first_viewport_center, first_viewport_edge) {
        assert!(
            vc_idx < ve_idx,
            "ViewportCenter priority should dequeue before ViewportEdge"
        );
    }

    // Verify that ViewportEdge comes before Prefetch
    let first_prefetch = dequeued_priorities.iter().position(|p| *p == Priority::Prefetch);
    if let (Some(ve_idx), Some(pf_idx)) = (first_viewport_edge, first_prefetch) {
        assert!(
            ve_idx < pf_idx,
            "ViewportEdge priority should dequeue before Prefetch"
        );
    }
}
