//! GPU-resident Bayer parity vs CPU reference.

use std::sync::{Arc, OnceLock};

use engine_gpu::resident::default_vram_config;
use engine_gpu::{
    compile_graph, palette_guided_params, palette_mixed_params_from_palette,
    palette_quantize_params_from_lut, void_and_cluster_ranks, BayerPassParams, CrosshatchPassParams,
    CrtPassParams, GpuCompositeFrameJob, GpuCompositeLayerOp, GpuCompositeTileWork, GpuContext,
    GpuExecutor, GpuFrameJob, GpuPipelineKey, GpuTileCache, GpuTileWork, GraphLayerFilter,
    HalftonePassParams, LineScreenPassParams, VoidAndClusterPassParams, WavePassParams,
};
use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey};

fn shared_test_ctx() -> Arc<GpuContext> {
    static CTX: OnceLock<Arc<GpuContext>> = OnceLock::new();
    Arc::clone(CTX.get_or_init(|| Arc::new(GpuContext::try_new_blocking().expect("adapter"))))
}

fn test_cache(ctx: &GpuContext) -> GpuTileCache {
    let mut cfg = default_vram_config();
    cfg.vram_budget_bytes = 64 * 1024 * 1024;
    cfg.frame_batch_cap = 8;
    GpuTileCache::new(&ctx.device, cfg)
}

fn dup_tile(src: &PixelTile) -> PixelTile {
    let mut t = PixelTile::new();
    t.copy_from(src);
    t
}

fn gradient_tile() -> PixelTile {
    let mut tile = PixelTile::new();
    for y in 0..256 {
        for x in 0..256 {
            let t = x as f32 / 255.0;
            tile.set(x + 2, y + 2, 0, t);
            tile.set(x + 2, y + 2, 1, t * 0.5);
            tile.set(x + 2, y + 2, 2, 1.0 - t);
            tile.set(x + 2, y + 2, 3, 1.0);
        }
    }
    tile
}

fn bayer_threshold_i32(gx: i32, gy: i32, matrix: u32) -> f32 {
    match matrix {
        2 => {
            let mx = (gx as i64).rem_euclid(2) as usize;
            let my = (gy as i64).rem_euclid(2) as usize;
            const M: [[f32; 2]; 2] = [[0.0 / 4.0, 2.0 / 4.0], [3.0 / 4.0, 1.0 / 4.0]];
            M[my][mx]
        }
        4 => {
            let mx = (gx as i64).rem_euclid(4) as usize;
            let my = (gy as i64).rem_euclid(4) as usize;
            const M: [[f32; 4]; 4] = [
                [0.0 / 16.0, 8.0 / 16.0, 2.0 / 16.0, 10.0 / 16.0],
                [12.0 / 16.0, 4.0 / 16.0, 14.0 / 16.0, 6.0 / 16.0],
                [3.0 / 16.0, 11.0 / 16.0, 1.0 / 16.0, 9.0 / 16.0],
                [15.0 / 16.0, 7.0 / 16.0, 13.0 / 16.0, 5.0 / 16.0],
            ];
            M[my][mx]
        }
        8 => {
            let mx = (gx as i64).rem_euclid(8) as usize;
            let my = (gy as i64).rem_euclid(8) as usize;
            const M: [[f32; 8]; 8] = [
                [
                    0.0 / 64.0,
                    32.0 / 64.0,
                    8.0 / 64.0,
                    40.0 / 64.0,
                    2.0 / 64.0,
                    34.0 / 64.0,
                    10.0 / 64.0,
                    42.0 / 64.0,
                ],
                [
                    48.0 / 64.0,
                    16.0 / 64.0,
                    56.0 / 64.0,
                    24.0 / 64.0,
                    50.0 / 64.0,
                    18.0 / 64.0,
                    58.0 / 64.0,
                    26.0 / 64.0,
                ],
                [
                    12.0 / 64.0,
                    44.0 / 64.0,
                    4.0 / 64.0,
                    36.0 / 64.0,
                    14.0 / 64.0,
                    46.0 / 64.0,
                    6.0 / 64.0,
                    38.0 / 64.0,
                ],
                [
                    60.0 / 64.0,
                    28.0 / 64.0,
                    52.0 / 64.0,
                    20.0 / 64.0,
                    62.0 / 64.0,
                    30.0 / 64.0,
                    54.0 / 64.0,
                    22.0 / 64.0,
                ],
                [
                    3.0 / 64.0,
                    35.0 / 64.0,
                    11.0 / 64.0,
                    43.0 / 64.0,
                    1.0 / 64.0,
                    33.0 / 64.0,
                    9.0 / 64.0,
                    41.0 / 64.0,
                ],
                [
                    51.0 / 64.0,
                    19.0 / 64.0,
                    59.0 / 64.0,
                    27.0 / 64.0,
                    49.0 / 64.0,
                    17.0 / 64.0,
                    57.0 / 64.0,
                    25.0 / 64.0,
                ],
                [
                    15.0 / 64.0,
                    47.0 / 64.0,
                    7.0 / 64.0,
                    39.0 / 64.0,
                    13.0 / 64.0,
                    45.0 / 64.0,
                    5.0 / 64.0,
                    37.0 / 64.0,
                ],
                [
                    63.0 / 64.0,
                    31.0 / 64.0,
                    55.0 / 64.0,
                    23.0 / 64.0,
                    61.0 / 64.0,
                    29.0 / 64.0,
                    53.0 / 64.0,
                    21.0 / 64.0,
                ],
            ];
            M[my][mx]
        }
        16 => {
            let mx = (gx as i64).rem_euclid(16) as usize;
            let my = (gy as i64).rem_euclid(16) as usize;
            const M: [u32; 256] = [
                0, 128, 32, 160, 8, 136, 40, 168, 2, 130, 34, 162, 10, 138, 42, 170, 192, 64, 224,
                96, 200, 72, 232, 104, 194, 66, 226, 98, 202, 74, 234, 106, 48, 176, 16, 144, 56,
                184, 24, 152, 50, 178, 18, 146, 58, 186, 26, 154, 240, 112, 208, 80, 248, 120, 216,
                88, 242, 114, 210, 82, 250, 122, 218, 90, 12, 140, 44, 172, 4, 132, 36, 164, 14,
                142, 46, 174, 6, 134, 38, 166, 204, 76, 236, 108, 196, 68, 228, 100, 206, 78, 238,
                110, 198, 70, 230, 102, 60, 188, 28, 156, 52, 180, 20, 148, 62, 190, 30, 158, 54,
                182, 22, 150, 252, 124, 220, 92, 244, 116, 212, 84, 254, 126, 222, 94, 246, 118,
                214, 86, 3, 131, 35, 163, 11, 139, 43, 171, 1, 129, 33, 161, 9, 137, 41, 169, 195,
                67, 227, 99, 203, 75, 235, 107, 193, 65, 225, 97, 201, 73, 233, 105, 51, 179, 19,
                147, 59, 187, 27, 155, 49, 177, 17, 145, 57, 185, 25, 153, 243, 115, 211, 83, 251,
                123, 219, 91, 241, 113, 209, 81, 249, 121, 217, 89, 15, 143, 47, 175, 7, 135, 39,
                167, 13, 141, 45, 173, 5, 133, 37, 165, 207, 79, 239, 111, 199, 71, 231, 103, 205,
                77, 237, 109, 197, 69, 229, 101, 63, 191, 31, 159, 55, 183, 23, 151, 61, 189, 29,
                157, 53, 181, 21, 149, 255, 127, 223, 95, 247, 119, 215, 87, 253, 125, 221, 93, 245,
                117, 213, 85,
            ];
            M[my * 16 + mx] as f32 / 256.0
        }
        108 => {
            let mx = (gx as i64).rem_euclid(8) as usize;
            let my = (gy as i64).rem_euclid(8) as usize;
            const M: [[u32; 8]; 8] = [
                [24, 10, 12, 26, 35, 47, 49, 37],
                [8, 0, 2, 14, 45, 59, 61, 51],
                [22, 6, 4, 16, 43, 57, 63, 53],
                [30, 20, 18, 28, 33, 41, 55, 39],
                [34, 46, 48, 36, 25, 11, 13, 27],
                [44, 58, 60, 50, 9, 1, 3, 15],
                [42, 56, 62, 52, 23, 7, 5, 17],
                [32, 40, 54, 38, 31, 21, 19, 29],
            ];
            M[my][mx] as f32 / 64.0
        }
        116 => {
            let mx = (gx as i64).rem_euclid(16) as usize;
            let my = (gy as i64).rem_euclid(16) as usize;
            const M: [u32; 256] = [
                1, 235, 59, 219, 15, 231, 55, 215, 2, 232, 56, 216, 12, 228, 52, 212, 129, 65, 187,
                123, 143, 79, 183, 119, 130, 66, 184, 120, 140, 76, 180, 116, 33, 193, 17, 251, 47,
                207, 31, 247, 34, 194, 18, 248, 44, 204, 28, 244, 161, 97, 145, 81, 175, 111, 159,
                95, 162, 98, 146, 82, 172, 108, 156, 92, 9, 225, 49, 209, 5, 239, 63, 223, 10, 226,
                50, 210, 6, 236, 60, 220, 137, 73, 177, 113, 133, 69, 191, 127, 138, 74, 178, 114,
                134, 70, 188, 124, 41, 201, 25, 241, 37, 197, 21, 255, 42, 202, 26, 242, 38, 198,
                22, 252, 169, 105, 153, 89, 165, 101, 149, 85, 170, 106, 154, 90, 166, 102, 150,
                86, 3, 233, 57, 217, 13, 229, 53, 213, 0, 234, 58, 218, 14, 230, 54, 214, 131, 67,
                185, 121, 141, 77, 181, 117, 128, 64, 186, 122, 142, 78, 182, 118, 35, 195, 19, 249,
                45, 205, 29, 245, 32, 192, 16, 250, 46, 206, 30, 246, 163, 99, 147, 83, 173, 109,
                157, 93, 160, 96, 144, 80, 174, 110, 158, 94, 11, 227, 51, 211, 7, 237, 61, 221, 8,
                224, 48, 208, 4, 238, 62, 222, 139, 75, 179, 115, 135, 71, 189, 125, 136, 72, 176,
                112, 132, 68, 190, 126, 43, 203, 27, 243, 39, 199, 23, 253, 40, 200, 24, 240, 36,
                196, 20, 254, 171, 107, 155, 91, 167, 103, 151, 87, 168, 104, 152, 88, 164, 100,
                148, 84,
            ];
            M[my * 16 + mx] as f32 / 256.0
        }
        _ => panic!("unsupported bayer matrix {matrix}"),
    }
}

fn bayer_threshold(gx: usize, gy: usize, matrix: u32) -> f32 {
    bayer_threshold_i32(gx as i32, gy as i32, matrix)
}

fn rotate_pattern_coord(gx: i32, gy: i32, angle_deg: f32) -> (i32, i32) {
    let wrapped = angle_deg.rem_euclid(360.0);
    if wrapped == 0.0 {
        return (gx, gy);
    }
    let theta = wrapped.to_radians();
    let (cos_t, sin_t) = (theta.cos(), theta.sin());
    let xr = gx as f32 * cos_t - gy as f32 * sin_t;
    let yr = gx as f32 * sin_t + gy as f32 * cos_t;
    (xr.floor() as i32, yr.floor() as i32)
}

fn apply_threshold_bias(threshold: f32, bias: f32) -> f32 {
    if bias == 0.0 {
        threshold
    } else {
        (threshold + bias).clamp(0.0, 0.999_999)
    }
}

fn cpu_bayer_core(
    input: &PixelTile,
    tile_x: u32,
    tile_y: u32,
    matrix: u32,
    levels: u16,
    threshold_scale: f32,
    threshold_bias: f32,
    pattern_angle: f32,
) -> PixelTile {
    let levels_f = levels as f32;
    let ox = tile_x * 256;
    let oy = tile_y * 256;
    let mut out = PixelTile::new();
    out.copy_from(input);
    for y in 0..256u32 {
        for x in 0..256u32 {
            let gx = (ox + x) as i32;
            let gy = (oy + y) as i32;
            let (pgx, pgy) = rotate_pattern_coord(gx, gy, pattern_angle);
            let t = apply_threshold_bias(bayer_threshold_i32(pgx, pgy, matrix), threshold_bias);
            let offset = (t - 0.5) * threshold_scale;
            let sx = x + 2;
            let sy = y + 2;
            for c in 0..3 {
                let v = input.at(sx, sy, c);
                let scaled = v * (levels_f - 1.0) + offset;
                out.set(
                    sx,
                    sy,
                    c,
                    scaled.round().clamp(0.0, levels_f - 1.0) / (levels_f - 1.0),
                );
            }
        }
    }
    out
}

fn cpu_vac_core(
    input: &PixelTile,
    tile_x: u32,
    tile_y: u32,
    levels: u16,
    threshold_scale: f32,
    threshold_bias: f32,
    pattern_angle: f32,
) -> PixelTile {
    let ranks = void_and_cluster_ranks();
    let n = ranks.len() as f32;
    let levels_f = levels as f32;
    let ox = tile_x * 256;
    let oy = tile_y * 256;
    let mut out = PixelTile::new();
    out.copy_from(input);
    for y in 0..256u32 {
        for x in 0..256u32 {
            let gx = (ox + x) as i32;
            let gy = (oy + y) as i32;
            let (pgx, pgy) = rotate_pattern_coord(gx, gy, pattern_angle);
            let mx = (pgx as i64).rem_euclid(64) as usize;
            let my = (pgy as i64).rem_euclid(64) as usize;
            let t = apply_threshold_bias(ranks[my * 64 + mx] as f32 / n, threshold_bias);
            let offset = (t - 0.5) * threshold_scale;
            let sx = x + 2;
            let sy = y + 2;
            for c in 0..3 {
                let v = input.at(sx, sy, c);
                let scaled = v * (levels_f - 1.0) + offset;
                out.set(
                    sx,
                    sy,
                    c,
                    scaled.round().clamp(0.0, levels_f - 1.0) / (levels_f - 1.0),
                );
            }
        }
    }
    out
}

fn run_resident_vac_parity(threshold_bias: f32, pattern_angle: f32) {
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");

    let raw = gradient_tile();
    let cpu = cpu_vac_core(&raw, 0, 0, 4, 1.0, threshold_bias, pattern_angle);

    let key = TileKey {
        doc: 1,
        layer: 1,
        coord: TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        stage: CacheStage::Processed,
    };

    let graph = Arc::new(
        compile_graph(&[GraphLayerFilter::VoidAndCluster(VoidAndClusterPassParams {
            levels: 4,
            threshold_scale: 1.0,
            color_mode: 0,
            threshold_bias,
            pattern_angle,
        })])
        .expect("graph"),
    );

    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph,
            speculative: false,
            tiles: vec![GpuTileWork {
                key,
                coord: key.coord,
                generation: 1,
                pixels: Arc::new(raw),
            }],
        })
        .expect("gpu submit");

    let gpu_tile = cache.demote(&ctx, &key).expect("demote").expect("slot");

    assert_tile_matches_cpu(&gpu_tile, &cpu);
    executor.shutdown();
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_vac_matches_cpu() {
    run_resident_vac_parity(0.0, 0.0);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_vac_threshold_bias() {
    run_resident_vac_parity(0.2, 0.0);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_vac_pattern_angle() {
    run_resident_vac_parity(0.0, 15.0);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_vac_seam_2x2() {
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");

    let raw = gradient_tile();
    let graph = Arc::new(
        compile_graph(&[GraphLayerFilter::VoidAndCluster(VoidAndClusterPassParams {
            levels: 4,
            threshold_scale: 1.0,
            color_mode: 0,
            threshold_bias: 0.0,
            pattern_angle: 0.0,
        })])
        .expect("graph"),
    );

    let tiles = [(0u32, 0u32), (1u32, 0u32), (0u32, 1u32), (1u32, 1u32)];
    let mut works = Vec::new();
    for (tx, ty) in tiles {
        let coord = TileCoord {
            level: 0,
            x: tx,
            y: ty,
        };
        let key = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Processed,
        };
        works.push(GpuTileWork {
            key,
            coord,
            generation: 1,
            pixels: Arc::new(dup_tile(&raw)),
        });
    }

    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph,
            speculative: false,
            tiles: works,
        })
        .expect("gpu submit");

    for (tx, ty) in tiles {
        let coord = TileCoord {
            level: 0,
            x: tx,
            y: ty,
        };
        let key = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Processed,
        };
        let cpu = cpu_vac_core(&raw, tx, ty, 4, 1.0, 0.0, 0.0);
        let gpu_tile = cache.demote(&ctx, &key).expect("demote").expect("slot");
        assert_tile_matches_cpu(&gpu_tile, &cpu);
    }
    executor.shutdown();
}

fn assert_tile_matches_cpu(gpu: &PixelTile, cpu: &PixelTile) {
    for y in 0..256 {
        for x in 0..256 {
            for c in 0..4 {
                assert_eq!(
                    cpu.at(x + 2, y + 2, c),
                    gpu.at(x + 2, y + 2, c),
                    "mismatch at ({x},{y}) ch {c}"
                );
            }
        }
    }
}

fn run_resident_bayer_parity(
    pipeline: GpuPipelineKey,
    matrix: u32,
    threshold_bias: f32,
    pattern_angle: f32,
) {
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");

    let raw = gradient_tile();
    let cpu = cpu_bayer_core(&raw, 0, 0, matrix, 4, 1.0, threshold_bias, pattern_angle);

    let key = TileKey {
        doc: 1,
        layer: 1,
        coord: TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        stage: CacheStage::Processed,
    };

    let graph = Arc::new(
        compile_graph(&[GraphLayerFilter::Bayer(BayerPassParams {
            pipeline,
            levels: 4,
            threshold_scale: 1.0,
            color_mode: 0,
            threshold_bias,
            pattern_angle,
        })])
        .expect("graph"),
    );

    if pattern_angle != 0.0 {
        if let engine_gpu::GraphNode::Gpu(pass) = &graph.nodes[0] {
            assert_eq!(pass.bayer.unwrap().pattern_angle, pattern_angle);
        }
    }

    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph,
            speculative: false,
            tiles: vec![GpuTileWork {
                key,
                coord: key.coord,
                generation: 1,
                pixels: Arc::new(raw),
            }],
        })
        .expect("gpu submit");

    let gpu_tile = cache.demote(&ctx, &key).expect("demote").expect("slot");

    assert_tile_matches_cpu(&gpu_tile, &cpu);
    executor.shutdown();
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_bayer2_matches_cpu() {
    run_resident_bayer_parity(GpuPipelineKey::Bayer2, 2, 0.0, 0.0);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_bayer4_matches_cpu() {
    run_resident_bayer_parity(GpuPipelineKey::Bayer4, 4, 0.0, 0.0);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_bayer8_matches_cpu() {
    run_resident_bayer_parity(GpuPipelineKey::Bayer8, 8, 0.0, 0.0);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_bayer16_matches_cpu() {
    run_resident_bayer_parity(GpuPipelineKey::Bayer16, 16, 0.0, 0.0);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_clustered_dot_matches_cpu() {
    run_resident_bayer_parity(GpuPipelineKey::ClusteredDot, 108, 0.0, 0.0);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_dispersed_dot_matches_cpu() {
    run_resident_bayer_parity(GpuPipelineKey::DispersedDot, 116, 0.0, 0.0);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_bayer4_threshold_bias() {
    run_resident_bayer_parity(GpuPipelineKey::Bayer4, 4, 0.2, 0.0);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_bayer4_pattern_angle() {
    run_resident_bayer_parity(GpuPipelineKey::Bayer4, 4, 0.0, 15.0);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_bayer4_seam_2x2() {
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");

    let raw = gradient_tile();
    let graph = Arc::new(
        compile_graph(&[GraphLayerFilter::Bayer(BayerPassParams {
            pipeline: GpuPipelineKey::Bayer4,
            levels: 4,
            threshold_scale: 1.0,
            color_mode: 0,
            threshold_bias: 0.0,
            pattern_angle: 0.0,
        })])
        .expect("graph"),
    );

    let tiles = [(0u32, 0u32), (1u32, 0u32), (0u32, 1u32), (1u32, 1u32)];
    let mut works = Vec::new();
    for (tx, ty) in tiles {
        let coord = TileCoord {
            level: 0,
            x: tx,
            y: ty,
        };
        let key = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Processed,
        };
        works.push(GpuTileWork {
            key,
            coord,
            generation: 1,
            pixels: Arc::new(dup_tile(&raw)),
        });
    }

    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph,
            speculative: false,
            tiles: works,
        })
        .expect("gpu submit");

    for (tx, ty) in tiles {
        let coord = TileCoord {
            level: 0,
            x: tx,
            y: ty,
        };
        let key = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Processed,
        };
        let cpu = cpu_bayer_core(&raw, tx, ty, 4, 4, 1.0, 0.0, 0.0);
        let gpu_tile = cache.demote(&ctx, &key).expect("demote").expect("slot");
        assert_tile_matches_cpu(&gpu_tile, &cpu);
    }
    executor.shutdown();
}

#[test]
fn cpu_angle_pixel_sanity() {
    let raw = gradient_tile();
    let rotated = cpu_bayer_core(&raw, 0, 0, 4, 4, 1.0, 0.0, 15.0);
    let unrot = cpu_bayer_core(&raw, 0, 0, 4, 4, 1.0, 0.0, 0.0);
    let rv = rotated.at(69, 140, 0);
    let uv = unrot.at(69, 140, 0);
    assert_ne!(rv, uv, "angle must change pixel (67,138)");
    assert_eq!(rv, 0.0, "rotated reference at (67,138)");
    assert_eq!(uv, 1.0 / 3.0, "unrotated reference at (67,138)");
}

const HALFTONE_PARITY_EPS: f32 = 1.0 / 255.0;

fn rem_euclid_f(a: f32, b: f32) -> f32 {
    let r = a % b;
    if r < 0.0 {
        r + b
    } else {
        r
    }
}

fn luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn cpu_halftone_core(
    input: &PixelTile,
    tile_x: u32,
    tile_y: u32,
    cell: f32,
    scale: f32,
    grayscale: bool,
) -> PixelTile {
    const ANGLES: [f32; 4] = [
        15.0_f32.to_radians(),
        75.0_f32.to_radians(),
        0.0,
        45.0_f32.to_radians(),
    ];
    let ox = tile_x * 256;
    let oy = tile_y * 256;
    let mut out = PixelTile::new();
    out.copy_from(input);
    for y in 0..256u32 {
        for x in 0..256u32 {
            let sx = x + 2;
            let sy = y + 2;
            let mut r = input.at(sx, sy, 0);
            let mut g = input.at(sx, sy, 1);
            let mut b = input.at(sx, sy, 2);
            if grayscale {
                let lum = luminance(r, g, b);
                r = lum;
                g = lum;
                b = lum;
            }
            let k = 1.0 - r.max(g).max(b);
            let (c, m, yk, kk) = if k >= 1.0 - f32::EPSILON {
                (0.0, 0.0, 0.0, 1.0)
            } else {
                (
                    ((1.0 - r - k) / (1.0 - k)).clamp(0.0, 1.0),
                    ((1.0 - g - k) / (1.0 - k)).clamp(0.0, 1.0),
                    ((1.0 - b - k) / (1.0 - k)).clamp(0.0, 1.0),
                    k.clamp(0.0, 1.0),
                )
            };
            let tones = [c, m, yk, kk];
            let gx = (ox + x) as f32;
            let gy = (oy + y) as f32;
            let mut dots = [0.0f32; 4];
            for ch in 0..4 {
                let cos_t = ANGLES[ch].cos();
                let sin_t = ANGLES[ch].sin();
                let xr = gx * cos_t + gy * sin_t;
                let yr = -gx * sin_t + gy * cos_t;
                let cx = rem_euclid_f(xr, cell) - cell * 0.5;
                let cy = rem_euclid_f(yr, cell) - cell * 0.5;
                let dist = (cx * cx + cy * cy).sqrt();
                let r_max = (cell * 0.5) * tones[ch].sqrt() * scale;
                dots[ch] = if dist <= r_max { 1.0 } else { 0.0 };
            }
            out.set(sx, sy, 0, 1.0 - (dots[0] + dots[3]).min(1.0));
            out.set(sx, sy, 1, 1.0 - (dots[1] + dots[3]).min(1.0));
            out.set(sx, sy, 2, 1.0 - (dots[2] + dots[3]).min(1.0));
        }
    }
    out
}

fn assert_tile_within_eps(gpu: &PixelTile, cpu: &PixelTile, eps: f32) {
    for y in 0..256 {
        for x in 0..256 {
            for c in 0..4 {
                let a = cpu.at(x + 2, y + 2, c);
                let b = gpu.at(x + 2, y + 2, c);
                assert!(
                    (a - b).abs() <= eps,
                    "mismatch at ({x},{y}) ch {c}: cpu={a} gpu={b} eps={eps}"
                );
            }
        }
    }
}

fn run_resident_halftone_parity(grayscale: bool) {
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");

    let raw = gradient_tile();
    let cpu = cpu_halftone_core(&raw, 0, 0, 8.0, 1.0, grayscale);

    let key = TileKey {
        doc: 1,
        layer: 1,
        coord: TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        stage: CacheStage::Processed,
    };

    let graph = Arc::new(
        compile_graph(&[GraphLayerFilter::Halftone(HalftonePassParams {
            cell_size: 8,
            threshold_scale: 1.0,
            dither_alpha: false,
            grayscale,
            angle_offset_deg: 0.0,
        })])
        .expect("graph"),
    );

    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph,
            speculative: false,
            tiles: vec![GpuTileWork {
                key,
                coord: key.coord,
                generation: 1,
                pixels: Arc::new(raw),
            }],
        })
        .expect("gpu submit");

    let gpu_tile = cache.demote(&ctx, &key).expect("demote").expect("slot");

    assert_tile_within_eps(&gpu_tile, &cpu, HALFTONE_PARITY_EPS);
    executor.shutdown();
}

fn cpu_line_screen_core(
    input: &PixelTile,
    tile_x: u32,
    tile_y: u32,
    cell: f32,
    angle: f32,
    scale: f32,
) -> PixelTile {
    let ox = tile_x * 256;
    let oy = tile_y * 256;
    let mut out = PixelTile::new();
    out.copy_from(input);
    for y in 0..256u32 {
        for x in 0..256u32 {
            let sx = x + 2;
            let sy = y + 2;
            let r = input.at(sx, sy, 0);
            let g = input.at(sx, sy, 1);
            let b = input.at(sx, sy, 2);
            let tone = (1.0 - luminance(r, g, b)).clamp(0.0, 1.0);
            let gx = (ox + x) as f32;
            let gy = (oy + y) as f32;
            let ink = if tone <= 0.0 {
                false
            } else {
                let s = cell.max(1.0);
                let phi = angle.to_radians();
                let u = gx * phi.cos() + gy * phi.sin();
                let d = rem_euclid_f(u, s);
                let dist = (d - s * 0.5).abs();
                dist <= (s * 0.5) * tone * scale.max(0.0)
            };
            let v = if ink { 0.0 } else { 1.0 };
            out.set(sx, sy, 0, v);
            out.set(sx, sy, 1, v);
            out.set(sx, sy, 2, v);
        }
    }
    out
}

fn cpu_crosshatch_core(
    input: &PixelTile,
    tile_x: u32,
    tile_y: u32,
    spacing: f32,
    angle: f32,
    thickness: f32,
) -> PixelTile {
    const LADDER: [(f32, f32, f32); 4] = [
        (0.12, 0.0, 1.0),
        (0.38, 0.0, 0.62),
        (0.58, 90.0, 0.62),
        (0.82, 45.0, 0.45),
    ];
    let ox = tile_x * 256;
    let oy = tile_y * 256;
    let base_spacing = spacing.max(2.0);
    let thickness_scale = thickness.max(0.05);
    let mut out = PixelTile::new();
    out.copy_from(input);
    for y in 0..256u32 {
        for x in 0..256u32 {
            let sx = x + 2;
            let sy = y + 2;
            let darkness = (1.0
                - luminance(
                    input.at(sx, sy, 0),
                    input.at(sx, sy, 1),
                    input.at(sx, sy, 2),
                ))
            .clamp(0.0, 1.0);
            let gx = (ox + x) as f32;
            let gy = (oy + y) as f32;
            let mut ink = false;
            for &(thresh, ang_off, sp_mul) in &LADDER {
                if darkness < thresh {
                    continue;
                }
                let sp = (base_spacing * sp_mul).max(2.0);
                let half_w = (sp * 0.15 * thickness_scale).clamp(0.35, sp * 0.45);
                let phi = (angle + ang_off).to_radians();
                let u = gx * phi.cos() + gy * phi.sin();
                let d = rem_euclid_f(u, sp);
                let dist = d.min(sp - d);
                if dist <= half_w.max(0.25) {
                    ink = true;
                    break;
                }
            }
            let v = if ink { 0.0 } else { 1.0 };
            out.set(sx, sy, 0, v);
            out.set(sx, sy, 1, v);
            out.set(sx, sy, 2, v);
        }
    }
    out
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_halftone_rgb_matches_cpu() {
    run_resident_halftone_parity(false);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_halftone_angled_differs_from_zero() {
    // Trig ULP at ink boundaries prevents bit-exact Rust↔WGSL for nonzero
    // offsets; prove the angle uniform is wired by differing from offset=0.
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");
    let raw = gradient_tile();
    let key0 = TileKey {
        doc: 1,
        layer: 1,
        coord: TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        stage: CacheStage::Processed,
    };
    let key15 = TileKey {
        doc: 1,
        layer: 1,
        coord: TileCoord {
            level: 0,
            x: 1,
            y: 0,
        },
        stage: CacheStage::Processed,
    };
    let g0 = Arc::new(
        compile_graph(&[GraphLayerFilter::Halftone(HalftonePassParams {
            cell_size: 8,
            threshold_scale: 1.0,
            dither_alpha: false,
            grayscale: false,
            angle_offset_deg: 0.0,
        })])
        .expect("graph"),
    );
    let g15 = Arc::new(
        compile_graph(&[GraphLayerFilter::Halftone(HalftonePassParams {
            cell_size: 8,
            threshold_scale: 1.0,
            dither_alpha: false,
            grayscale: false,
            angle_offset_deg: 15.0,
        })])
        .expect("graph"),
    );
    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph: g0,
            speculative: false,
            tiles: vec![GpuTileWork {
                key: key0,
                coord: key0.coord,
                generation: 1,
                pixels: Arc::new(dup_tile(&raw)),
            }],
        })
        .expect("gpu submit 0");
    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph: g15,
            speculative: false,
            tiles: vec![GpuTileWork {
                key: key15,
                coord: key15.coord,
                generation: 1,
                pixels: Arc::new(dup_tile(&raw)),
            }],
        })
        .expect("gpu submit 15");
    let t0 = cache.demote(&ctx, &key0).expect("demote").expect("slot0");
    let t15 = cache.demote(&ctx, &key15).expect("demote").expect("slot15");
    let mut differed = false;
    'scan: for y in 0..256 {
        for x in 0..256 {
            for c in 0..3 {
                if t0.at(x + 2, y + 2, c) != t15.at(x + 2, y + 2, c) {
                    differed = true;
                    break 'scan;
                }
            }
        }
    }
    assert!(differed, "angle_offset_deg=15 must change output vs 0");
    executor.shutdown();
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_line_screen_matches_cpu() {
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");
    let raw = gradient_tile();
    let cpu = cpu_line_screen_core(&raw, 0, 0, 8.0, 45.0, 1.0);
    let key = TileKey {
        doc: 1,
        layer: 1,
        coord: TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        stage: CacheStage::Processed,
    };
    let graph = Arc::new(
        compile_graph(&[GraphLayerFilter::LineScreen(LineScreenPassParams {
            cell_size: 8,
            threshold_scale: 1.0,
            pattern_angle: 45.0,
            dither_alpha: false,
        })])
        .expect("graph"),
    );
    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph,
            speculative: false,
            tiles: vec![GpuTileWork {
                key,
                coord: key.coord,
                generation: 1,
                pixels: Arc::new(raw),
            }],
        })
        .expect("gpu submit");
    let gpu_tile = cache.demote(&ctx, &key).expect("demote").expect("slot");
    assert_tile_matches_cpu(&gpu_tile, &cpu);
    executor.shutdown();
}

fn cpu_wave_core(
    input: &PixelTile,
    tile_x: u32,
    tile_y: u32,
    levels: u16,
    threshold_scale: f32,
    wavelength: f32,
    amplitude: f32,
    phase: f32,
    wave_angle: f32,
) -> PixelTile {
    let levels_f = levels as f32;
    let ox = tile_x * 256;
    let oy = tile_y * 256;
    let wl = wavelength.max(1e-6);
    let phi = wave_angle.to_radians();
    let mut out = PixelTile::new();
    out.copy_from(input);
    for y in 0..256u32 {
        for x in 0..256u32 {
            let gx = (ox + x) as f32;
            let gy = (oy + y) as f32;
            let u = gx * phi.cos() + gy * phi.sin();
            let t = (0.5 + 0.5 * (std::f32::consts::TAU * u / wl + phase).sin() * amplitude)
                .clamp(0.0, 0.999_999);
            let offset = (t - 0.5) * threshold_scale;
            let sx = x + 2;
            let sy = y + 2;
            for c in 0..3 {
                let v = input.at(sx, sy, c);
                let scaled = v * (levels_f - 1.0) + offset;
                out.set(
                    sx,
                    sy,
                    c,
                    scaled.round().clamp(0.0, levels_f - 1.0) / (levels_f - 1.0),
                );
            }
        }
    }
    out
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_wave_matches_cpu() {
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");
    let raw = gradient_tile();
    let cpu = cpu_wave_core(&raw, 0, 0, 4, 1.0, 8.0, 1.0, 0.0, 0.0);
    let key = TileKey {
        doc: 1,
        layer: 1,
        coord: TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        stage: CacheStage::Processed,
    };
    let graph = Arc::new(
        compile_graph(&[GraphLayerFilter::Wave(WavePassParams {
            levels: 4,
            threshold_scale: 1.0,
            color_mode: 0,
            threshold_bias: 0.0,
            wavelength: 8.0,
            amplitude: 1.0,
            phase: 0.0,
            wave_angle: 0.0,
        })])
        .expect("graph"),
    );
    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph,
            speculative: false,
            tiles: vec![GpuTileWork {
                key,
                coord: key.coord,
                generation: 1,
                pixels: Arc::new(raw),
            }],
        })
        .expect("gpu submit");
    let gpu_tile = cache.demote(&ctx, &key).expect("demote").expect("slot");
    assert_tile_matches_cpu(&gpu_tile, &cpu);
    executor.shutdown();
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_crosshatch_matches_cpu() {
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");
    let raw = gradient_tile();
    let cpu = cpu_crosshatch_core(&raw, 0, 0, 8.0, 45.0, 1.0);
    let key = TileKey {
        doc: 1,
        layer: 1,
        coord: TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        stage: CacheStage::Processed,
    };
    let graph = Arc::new(
        compile_graph(&[GraphLayerFilter::Crosshatch(CrosshatchPassParams {
            spacing: 8.0,
            threshold_scale: 1.0,
            pattern_angle: 45.0,
            dither_alpha: false,
        })])
        .expect("graph"),
    );
    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph,
            speculative: false,
            tiles: vec![GpuTileWork {
                key,
                coord: key.coord,
                generation: 1,
                pixels: Arc::new(raw),
            }],
        })
        .expect("gpu submit");
    let gpu_tile = cache.demote(&ctx, &key).expect("demote").expect("slot");
    assert_tile_matches_cpu(&gpu_tile, &cpu);
    executor.shutdown();
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_halftone_gray_matches_cpu() {
    run_resident_halftone_parity(true);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_halftone_seam_2x1() {
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");

    let raw = gradient_tile();
    let graph = Arc::new(
        compile_graph(&[GraphLayerFilter::Halftone(HalftonePassParams {
            cell_size: 8,
            threshold_scale: 1.0,
            dither_alpha: false,
            grayscale: false,
            angle_offset_deg: 0.0,
        })])
        .expect("graph"),
    );

    let tiles = [(0u32, 0u32), (1u32, 0u32)];
    let mut works = Vec::new();
    for (tx, ty) in tiles {
        let coord = TileCoord {
            level: 0,
            x: tx,
            y: ty,
        };
        let key = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Processed,
        };
        works.push(GpuTileWork {
            key,
            coord,
            generation: 1,
            pixels: Arc::new(dup_tile(&raw)),
        });
    }

    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph,
            speculative: false,
            tiles: works,
        })
        .expect("gpu submit");

    for (tx, ty) in tiles {
        let coord = TileCoord {
            level: 0,
            x: tx,
            y: ty,
        };
        let key = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Processed,
        };
        let cpu = cpu_halftone_core(&raw, tx, ty, 8.0, 1.0, false);
        let gpu_tile = cache.demote(&ctx, &key).expect("demote").expect("slot");
        assert_tile_within_eps(&gpu_tile, &cpu, HALFTONE_PARITY_EPS);
    }
    executor.shutdown();
}

const CRT_PARITY_EPS: f32 = 1.0 / 255.0;

fn solid_tile(v: f32) -> PixelTile {
    let mut tile = PixelTile::new();
    for y in 0..256 {
        for x in 0..256 {
            tile.set(x + 2, y + 2, 0, v);
            tile.set(x + 2, y + 2, 1, v);
            tile.set(x + 2, y + 2, 2, v);
            tile.set(x + 2, y + 2, 3, 1.0);
        }
    }
    tile
}

fn rem_euclid_i(a: i32, b: i32) -> i32 {
    let r = a % b;
    if r < 0 {
        r + b
    } else {
        r
    }
}

fn cpu_crt_core(
    input: &PixelTile,
    tile_x: u32,
    tile_y: u32,
    period: u8,
    strength: f32,
    mask_strength: f32,
) -> PixelTile {
    let ox = (tile_x * 256) as i32;
    let oy = (tile_y * 256) as i32;
    let p = period as i32;
    let mut out = PixelTile::new();
    out.copy_from(input);
    for y in 0..256u32 {
        for x in 0..256u32 {
            let gx = ox + x as i32;
            let gy = oy + y as i32;
            let line = rem_euclid_i(gy, p);
            let dark_rows = (p / 2).max(1);
            let gain = if line < dark_rows {
                1.0 - strength
            } else {
                1.0
            };
            let sx = x + 2;
            let sy = y + 2;
            for c in 0..3u32 {
                let mask = if mask_strength <= 0.0 {
                    1.0
                } else {
                    let col = rem_euclid_i(gx, 3) as u32;
                    if col == c {
                        1.0
                    } else {
                        1.0 - mask_strength
                    }
                };
                let v = (input.at(sx, sy, c) * gain * mask).clamp(0.0, 1.0);
                out.set(sx, sy, c, v);
            }
        }
    }
    out
}

fn run_resident_crt_parity(period: u8, strength: f32, mask_strength: f32) {
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");

    let raw = solid_tile(0.8);
    let cpu = cpu_crt_core(&raw, 0, 0, period, strength, mask_strength);

    let key = TileKey {
        doc: 1,
        layer: 1,
        coord: TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        stage: CacheStage::Processed,
    };

    let graph = Arc::new(
        compile_graph(&[GraphLayerFilter::Crt(CrtPassParams {
            period,
            strength,
            mask_strength,
        })])
        .expect("graph"),
    );

    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph,
            speculative: false,
            tiles: vec![GpuTileWork {
                key,
                coord: key.coord,
                generation: 1,
                pixels: Arc::new(raw),
            }],
        })
        .expect("gpu submit");

    let gpu_tile = cache.demote(&ctx, &key).expect("demote").expect("slot");

    assert_tile_within_eps(&gpu_tile, &cpu, CRT_PARITY_EPS);
    executor.shutdown();
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_crt_scanline_matches_cpu() {
    run_resident_crt_parity(2, 0.5, 0.0);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_crt_mask_matches_cpu() {
    run_resident_crt_parity(2, 0.5, 0.25);
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_crt_horizontal_seam() {
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");

    let raw = solid_tile(0.8);
    let graph = Arc::new(
        compile_graph(&[GraphLayerFilter::Crt(CrtPassParams {
            period: 2,
            strength: 0.5,
            mask_strength: 0.0,
        })])
        .expect("graph"),
    );

    let tiles = [(0u32, 0u32), (0u32, 1u32)];
    let mut works = Vec::new();
    for (tx, ty) in tiles {
        let coord = TileCoord {
            level: 0,
            x: tx,
            y: ty,
        };
        let key = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Processed,
        };
        works.push(GpuTileWork {
            key,
            coord,
            generation: 1,
            pixels: Arc::new(dup_tile(&raw)),
        });
    }

    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph,
            speculative: false,
            tiles: works,
        })
        .expect("gpu submit");

    for (tx, ty) in tiles {
        let coord = TileCoord {
            level: 0,
            x: tx,
            y: ty,
        };
        let key = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Processed,
        };
        let cpu = cpu_crt_core(&raw, tx, ty, 2, 0.5, 0.0);
        let gpu_tile = cache.demote(&ctx, &key).expect("demote").expect("slot");
        assert_tile_within_eps(&gpu_tile, &cpu, CRT_PARITY_EPS);
    }

    // Seam continuity: global Y=255 (odd→bright) vs Y=256 (even→dark) with period=2.
    let top = cpu_crt_core(&raw, 0, 0, 2, 0.5, 0.0);
    let bot = cpu_crt_core(&raw, 0, 1, 2, 0.5, 0.0);
    assert!((top.at(2, 2 + 255, 0) - 0.8).abs() < CRT_PARITY_EPS);
    assert!((bot.at(2, 2, 0) - 0.4).abs() < CRT_PARITY_EPS);
    executor.shutdown();
}

fn build_test_palette_params(lut_size: u32) -> engine_gpu::PaletteQuantizePassParams {
    use engine_color::kdtree::KdTree;
    use engine_color::oklab::{linear_to_oklab, LinRgb};
    use engine_color::palette::{LinearColor, Palette};
    use engine_color::palette_lut::PaletteLut3D;

    let colors = vec![
        LinearColor {
            r: 0.0,
            g: 0.0,
            b: 0.0,
        },
        LinearColor {
            r: 1.0,
            g: 0.0,
            b: 0.0,
        },
        LinearColor {
            r: 0.0,
            g: 1.0,
            b: 0.0,
        },
        LinearColor {
            r: 0.0,
            g: 0.0,
            b: 1.0,
        },
        LinearColor {
            r: 1.0,
            g: 1.0,
            b: 1.0,
        },
    ];
    let palette = Palette {
        id: 1,
        name: "test".into(),
        colors,
        revision: 1,
    };
    let labs: Vec<_> = palette
        .colors
        .iter()
        .map(|c| {
            linear_to_oklab(LinRgb {
                r: c.r,
                g: c.g,
                b: c.b,
            })
        })
        .collect();
    let tree = KdTree::build(&labs).expect("kd");
    let lut = PaletteLut3D::build(&palette, lut_size, &tree).expect("lut");
    palette_quantize_params_from_lut(&lut, &palette)
}

fn cpu_palette_quantize_core(
    input: &PixelTile,
    params: &engine_gpu::PaletteQuantizePassParams,
) -> PixelTile {
    use engine_color::oklab::{linear_to_oklab, LinRgb};

    fn axis_index(v: f32, range: (f32, f32), n: usize) -> usize {
        let (lo, hi) = range;
        let span = hi - lo;
        if span <= 0.0 {
            return 0;
        }
        let t = ((v - lo) / span).clamp(0.0, 1.0 - f32::EPSILON);
        let i = (t * n as f32).floor() as usize;
        i.min(n - 1)
    }

    let n = params.lut_size as usize;
    let mut out = PixelTile::new();
    out.copy_from(input);
    for y in 0..256u32 {
        for x in 0..256u32 {
            let sx = x + 2;
            let sy = y + 2;
            let r = input.at(sx, sy, 0);
            let g = input.at(sx, sy, 1);
            let b = input.at(sx, sy, 2);
            let lab = linear_to_oklab(LinRgb { r, g, b });
            let i = axis_index(lab.l, params.l_range, n);
            let j = axis_index(lab.a, params.a_range, n);
            let k = axis_index(lab.b, params.b_range, n);
            let flat = (i * n + j) * n + k;
            let idx = params.lut_grid[flat] as usize;
            let c = params.palette_rgb[idx.min(params.palette_rgb.len() - 1)];
            out.set(sx, sy, 0, c[0]);
            out.set(sx, sy, 1, c[1]);
            out.set(sx, sy, 2, c[2]);
        }
    }
    out
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_palette_quantize_matches_cpu() {
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");

    let params = build_test_palette_params(16);
    let raw = gradient_tile();
    let cpu = cpu_palette_quantize_core(&raw, &params);

    let key = TileKey {
        doc: 1,
        layer: 1,
        coord: TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        stage: CacheStage::Processed,
    };

    let graph =
        Arc::new(compile_graph(&[GraphLayerFilter::PaletteQuantize(params)]).expect("graph"));

    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph,
            speculative: false,
            tiles: vec![GpuTileWork {
                key,
                coord: key.coord,
                generation: 1,
                pixels: Arc::new(raw),
            }],
        })
        .expect("gpu submit");

    let gpu_tile = cache.demote(&ctx, &key).expect("demote").expect("slot");

    // Oklab cbrt can differ by 1 ULP across CPU/GPU → allow tiny channel eps.
    assert_tile_within_eps(&gpu_tile, &cpu, 1e-5);
    executor.shutdown();
}

fn test_bw_palette() -> engine_color::palette::Palette {
    use engine_color::palette::{LinearColor, Palette};
    Palette {
        id: 1,
        name: "bw".into(),
        colors: vec![
            LinearColor {
                r: 0.0,
                g: 0.0,
                b: 0.0,
            },
            LinearColor {
                r: 1.0,
                g: 1.0,
                b: 1.0,
            },
            LinearColor {
                r: 1.0,
                g: 0.2,
                b: 0.4,
            },
            LinearColor {
                r: 0.2,
                g: 0.6,
                b: 1.0,
            },
        ],
        revision: 1,
    }
}

fn cpu_guided_core(
    input: &PixelTile,
    tile_x: u32,
    tile_y: u32,
    matrix: u32,
    params: &engine_gpu::PaletteGuidedPassParams,
) -> PixelTile {
    use engine_color::palette_guided::{quantize_channel_guided, ChannelRange};

    let ranges = [
        ChannelRange {
            min: params.ranges[0][0],
            max: params.ranges[0][1],
        },
        ChannelRange {
            min: params.ranges[1][0],
            max: params.ranges[1][1],
        },
        ChannelRange {
            min: params.ranges[2][0],
            max: params.ranges[2][1],
        },
    ];
    let ox = tile_x * 256;
    let oy = tile_y * 256;
    let mut out = PixelTile::new();
    out.copy_from(input);
    for y in 0..256u32 {
        for x in 0..256u32 {
            let gx = (ox + x) as i32;
            let gy = (oy + y) as i32;
            let threshold = bayer_threshold_i32(gx, gy, matrix);
            let t = 0.5 + (threshold - 0.5) * params.threshold_scale;
            let sx = x + 2;
            let sy = y + 2;
            let r = input.at(sx, sy, 0);
            let g = input.at(sx, sy, 1);
            let b = input.at(sx, sy, 2);
            let is_gray = params.color_mode % 2 == 1;
            if !is_gray {
                out.set(
                    sx,
                    sy,
                    0,
                    quantize_channel_guided(r, ranges[0], params.channel_levels, t),
                );
                out.set(
                    sx,
                    sy,
                    1,
                    quantize_channel_guided(g, ranges[1], params.channel_levels, t),
                );
                out.set(
                    sx,
                    sy,
                    2,
                    quantize_channel_guided(b, ranges[2], params.channel_levels, t),
                );
            } else {
                let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                out.set(
                    sx,
                    sy,
                    0,
                    quantize_channel_guided(lum, ranges[0], params.channel_levels, t),
                );
                out.set(
                    sx,
                    sy,
                    1,
                    quantize_channel_guided(lum, ranges[1], params.channel_levels, t),
                );
                out.set(
                    sx,
                    sy,
                    2,
                    quantize_channel_guided(lum, ranges[2], params.channel_levels, t),
                );
            }
        }
    }
    out
}

fn cpu_ordered_two_nearest(
    r: f32,
    g: f32,
    b: f32,
    threshold: f32,
    threshold_scale: f32,
    rgb: &[[f32; 3]],
    lab: &[[f32; 3]],
) -> (f32, f32, f32) {
    use engine_color::oklab::{linear_to_oklab, LinRgb, Oklab};

    let n = lab.len();
    if n == 0 {
        return (r, g, b);
    }
    if n == 1 {
        return (rgb[0][0], rgb[0][1], rgb[0][2]);
    }
    let query = linear_to_oklab(LinRgb { r, g, b });
    let dist = |i: usize| {
        let o = Oklab {
            l: lab[i][0],
            a: lab[i][1],
            b: lab[i][2],
        };
        let dl = query.l - o.l;
        let da = query.a - o.a;
        let db = query.b - o.b;
        dl * dl + da * da + db * db
    };
    let mut i1 = 0usize;
    let mut i2 = 1usize;
    let mut d1 = dist(0);
    let mut d2 = dist(1);
    if d2 < d1 {
        std::mem::swap(&mut i1, &mut i2);
        std::mem::swap(&mut d1, &mut d2);
    }
    for i in 2..n {
        let d = dist(i);
        if d < d1 {
            d2 = d1;
            i2 = i1;
            d1 = d;
            i1 = i;
        } else if d < d2 {
            d2 = d;
            i2 = i;
        }
    }
    let mix = if d1 + d2 <= f32::EPSILON {
        0.0
    } else {
        let sd1 = d1.sqrt();
        let sd2 = d2.sqrt();
        sd1 / (sd1 + sd2)
    };
    let t = 0.5 + (threshold - 0.5) * threshold_scale;
    let idx = if t < mix { i2 } else { i1 };
    (rgb[idx][0], rgb[idx][1], rgb[idx][2])
}

fn cpu_mixed_core(
    input: &PixelTile,
    tile_x: u32,
    tile_y: u32,
    matrix: u32,
    mixed: &engine_gpu::PaletteMixedPassParams,
) -> PixelTile {
    let guided = cpu_guided_core(input, tile_x, tile_y, matrix, &mixed.guided);
    let ox = tile_x * 256;
    let oy = tile_y * 256;
    let mut out = PixelTile::new();
    out.copy_from(&guided);
    for y in 0..256u32 {
        for x in 0..256u32 {
            let gx = (ox + x) as i32;
            let gy = (oy + y) as i32;
            let threshold = bayer_threshold_i32(gx, gy, matrix);
            let sx = x + 2;
            let sy = y + 2;
            let (sr, sg, sb) = cpu_ordered_two_nearest(
                guided.at(sx, sy, 0),
                guided.at(sx, sy, 1),
                guided.at(sx, sy, 2),
                threshold,
                mixed.guided.threshold_scale,
                &mixed.palette_rgb,
                &mixed.palette_lab,
            );
            out.set(sx, sy, 0, sr);
            out.set(sx, sy, 1, sg);
            out.set(sx, sy, 2, sb);
        }
    }
    out
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_palette_guided_matches_cpu() {
    use engine_color::palette_guided::palette_channel_ranges;

    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");

    let palette = test_bw_palette();
    let ranges = palette_channel_ranges(&palette);
    let params = palette_guided_params(GpuPipelineKey::Bayer4, 4, 1.0, 0, 0.0, 0.0, ranges);
    let raw = gradient_tile();
    let cpu = cpu_guided_core(&raw, 0, 0, 4, &params);

    let key = TileKey {
        doc: 1,
        layer: 1,
        coord: TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        stage: CacheStage::Processed,
    };
    let graph = Arc::new(compile_graph(&[GraphLayerFilter::PaletteGuided(params)]).expect("graph"));
    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph,
            speculative: false,
            tiles: vec![GpuTileWork {
                key,
                coord: key.coord,
                generation: 1,
                pixels: Arc::new(raw),
            }],
        })
        .expect("gpu submit");
    let gpu_tile = cache.demote(&ctx, &key).expect("demote").expect("slot");
    assert_tile_matches_cpu(&gpu_tile, &cpu);
    executor.shutdown();
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_palette_mixed_matches_cpu() {
    use engine_color::palette_guided::palette_channel_ranges;

    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");

    let palette = test_bw_palette();
    let ranges = palette_channel_ranges(&palette);
    let guided = palette_guided_params(GpuPipelineKey::Bayer4, 4, 1.0, 0, 0.0, 0.0, ranges);
    let mixed = palette_mixed_params_from_palette(guided, &palette);
    let raw = gradient_tile();
    let cpu = cpu_mixed_core(&raw, 0, 0, 4, &mixed);

    let key = TileKey {
        doc: 1,
        layer: 1,
        coord: TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        stage: CacheStage::Processed,
    };
    let graph = Arc::new(compile_graph(&[GraphLayerFilter::PaletteMixed(mixed)]).expect("graph"));
    executor
        .submit_frame_blocking(GpuFrameJob {
            doc_gen: 1,
            graph,
            speculative: false,
            tiles: vec![GpuTileWork {
                key,
                coord: key.coord,
                generation: 1,
                pixels: Arc::new(raw),
            }],
        })
        .expect("gpu submit");
    let gpu_tile = cache.demote(&ctx, &key).expect("demote").expect("slot");
    // Oklab + sqrt mix can be 1 ULP off vs CPU.
    assert_tile_within_eps(&gpu_tile, &cpu, 1e-5);
    executor.shutdown();
}

fn solid_core_tile(r: f32, g: f32, b: f32, a: f32) -> PixelTile {
    let mut tile = PixelTile::new();
    for y in 0..256u32 {
        for x in 0..256u32 {
            tile.set(x + 2, y + 2, 0, r);
            tile.set(x + 2, y + 2, 1, g);
            tile.set(x + 2, y + 2, 2, b);
            tile.set(x + 2, y + 2, 3, a);
        }
    }
    tile
}

fn apply_blend_mode_cpu(mode: u32, s: f32, d: f32) -> f32 {
    match mode {
        1 => s * d,
        2 => s + d - s * d,
        3 => {
            if d < 0.5 {
                2.0 * s * d
            } else {
                1.0 - 2.0 * (1.0 - s) * (1.0 - d)
            }
        }
        4 => s.min(d),
        5 => s.max(d),
        6 => {
            if s >= 1.0 {
                1.0
            } else {
                (d / (1.0 - s)).min(1.0)
            }
        }
        7 => {
            if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - d) / s).min(1.0)
            }
        }
        8 => {
            if s < 0.5 {
                2.0 * s * d
            } else {
                1.0 - 2.0 * (1.0 - s) * (1.0 - d)
            }
        }
        9 => {
            let dd = if d <= 0.25 {
                ((16.0 * d - 12.0) * d + 4.0) * d
            } else {
                d.sqrt()
            };
            if s <= 0.5 {
                d - (1.0 - 2.0 * s) * d * (1.0 - d)
            } else {
                d + (2.0 * s - 1.0) * (dd - d)
            }
        }
        10 => (s - d).abs(),
        11 => s + d - 2.0 * s * d,
        _ => s,
    }
}

fn cpu_blend_tile(dst: &mut PixelTile, src: &PixelTile, mode: u32, opacity: f32) {
    for y in 2..(2 + 256) {
        for x in 2..(2 + 256) {
            let src_a = src.at(x, y, 3) * opacity;
            if src_a < 1e-6 {
                continue;
            }
            let dst_a = dst.at(x, y, 3);
            for c in 0..3 {
                let s = src.at(x, y, c);
                let d = dst.at(x, y, c);
                let blended = apply_blend_mode_cpu(mode, s, d);
                dst.set(x, y, c, blended * src_a + d * dst_a * (1.0 - src_a));
            }
            dst.set(x, y, 3, src_a + dst_a * (1.0 - src_a));
        }
    }
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_composite_two_layers_normal() {
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");

    let bottom = Arc::new(solid_core_tile(0.0, 1.0, 0.0, 1.0));
    let top = Arc::new(solid_core_tile(1.0, 0.0, 0.0, 1.0));
    let mut cpu = PixelTile::new();
    cpu_blend_tile(&mut cpu, &bottom, 0, 1.0);
    cpu_blend_tile(&mut cpu, &top, 0, 0.5);

    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    let bottom_key = TileKey {
        doc: 1,
        layer: 1,
        coord,
        stage: CacheStage::Processed,
    };
    let top_key = TileKey {
        doc: 1,
        layer: 2,
        coord,
        stage: CacheStage::Processed,
    };
    let composite_key = TileKey {
        doc: 1,
        layer: 0,
        coord,
        stage: CacheStage::Composite,
    };

    executor
        .submit_composite_blocking(GpuCompositeFrameJob {
            doc_gen: 1,
            speculative: false,
            tiles: vec![GpuCompositeTileWork {
                coord,
                composite_key,
                generation: 1,
                layers: vec![
                    GpuCompositeLayerOp {
                        processed_key: bottom_key,
                        blend_mode: 0,
                        opacity: 1.0,
                        pixels: Some(Arc::clone(&bottom)),
                    },
                    GpuCompositeLayerOp {
                        processed_key: top_key,
                        blend_mode: 0,
                        opacity: 0.5,
                        pixels: Some(Arc::clone(&top)),
                    },
                ],
            }],
        })
        .expect("gpu submit");

    let gpu = cache
        .demote(&ctx, &composite_key)
        .expect("demote")
        .expect("slot");
    assert_tile_matches_cpu(&gpu, &cpu);
    executor.shutdown();
}

#[test]
#[ignore = "requires GPU adapter"]
fn resident_composite_three_layers_multiply_screen() {
    let ctx = shared_test_ctx();
    let cache = Arc::new(test_cache(&ctx));
    let mut executor = GpuExecutor::spawn(Arc::clone(&ctx), Arc::clone(&cache)).expect("executor");

    let l0 = Arc::new(solid_core_tile(0.8, 0.8, 0.8, 1.0));
    let l1 = Arc::new(solid_core_tile(0.5, 0.2, 0.2, 1.0));
    let l2 = Arc::new(solid_core_tile(0.2, 0.5, 0.9, 0.75));
    let mut cpu = PixelTile::new();
    cpu_blend_tile(&mut cpu, &l0, 0, 1.0); // Normal
    cpu_blend_tile(&mut cpu, &l1, 1, 1.0); // Multiply
    cpu_blend_tile(&mut cpu, &l2, 2, 0.8); // Screen @ 0.8

    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    let keys: Vec<TileKey> = (1..=3)
        .map(|layer| TileKey {
            doc: 1,
            layer,
            coord,
            stage: CacheStage::Processed,
        })
        .collect();
    let composite_key = TileKey {
        doc: 1,
        layer: 0,
        coord,
        stage: CacheStage::Composite,
    };
    let pixels = [Arc::clone(&l0), Arc::clone(&l1), Arc::clone(&l2)];
    let modes = [(0, 1.0f32), (1, 1.0), (2, 0.8)];

    executor
        .submit_composite_blocking(GpuCompositeFrameJob {
            doc_gen: 1,
            speculative: false,
            tiles: vec![GpuCompositeTileWork {
                coord,
                composite_key,
                generation: 1,
                layers: keys
                    .iter()
                    .zip(pixels.iter())
                    .zip(modes.iter())
                    .map(|((key, pix), (mode, opac))| GpuCompositeLayerOp {
                        processed_key: *key,
                        blend_mode: *mode,
                        opacity: *opac,
                        pixels: Some(Arc::clone(pix)),
                    })
                    .collect(),
            }],
        })
        .expect("gpu submit");

    let gpu = cache
        .demote(&ctx, &composite_key)
        .expect("demote")
        .expect("slot");
    // Multi-pass Screen/Multiply can drift 1–2 ULP vs scalar CPU.
    assert_tile_within_eps(&gpu, &cpu, 1e-5);
    executor.shutdown();
}
