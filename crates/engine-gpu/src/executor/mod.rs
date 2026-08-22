//! Dedicated GPU executor thread — one submit per frame (Path B D4).

use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crossbeam_channel::{bounded, Receiver, Sender};
use engine_tiles::{PixelTile, TileCoord, TileKey};

use crate::composite::GpuCompositeFrameJob;
use crate::context::GpuContext;
use crate::graph::{ComputeGraph, GraphNode, GpuPipelineKey};
use crate::resident::{
    GpuTileCache, ReadbackRing, ResidentBayerPipelines, ResidentCompositePipelines,
    ResidentCrtPipelines, ResidentGatherPipelines, ResidentHalftonePipelines,
    ResidentPaletteGuidedPipelines, ResidentPalettePipelines,
};
use crate::GpuError;

const QUEUE_DEPTH: usize = 2;

/// One tile processed in a resident frame batch.
#[derive(Clone)]
pub struct GpuTileWork {
    pub key: TileKey,
    pub coord: TileCoord,
    pub generation: u64,
    pub pixels: Arc<PixelTile>,
}

impl std::fmt::Debug for GpuTileWork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuTileWork")
            .field("key", &self.key)
            .field("coord", &self.coord)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub struct GpuFrameJob {
    pub doc_gen: u64,
    pub graph: Arc<ComputeGraph>,
    pub tiles: Vec<GpuTileWork>,
}

enum ExecutorMsg {
    Frame(GpuFrameJob),
    FrameWithAck {
        job: GpuFrameJob,
        done_tx: std::sync::mpsc::Sender<Result<(), GpuError>>,
    },
    Composite(GpuCompositeFrameJob),
    CompositeWithAck {
        job: GpuCompositeFrameJob,
        done_tx: std::sync::mpsc::Sender<Result<(), GpuError>>,
    },
    Shutdown,
}

pub struct GpuExecutor {
    tx: Sender<ExecutorMsg>,
    join: Option<JoinHandle<()>>,
}

impl GpuExecutor {
    pub fn spawn(
        ctx: Arc<GpuContext>,
        cache: Arc<GpuTileCache>,
    ) -> Result<Self, GpuError> {
        let bayer = ResidentBayerPipelines::create(&ctx.device)?;
        let halftone = ResidentHalftonePipelines::create(&ctx.device)?;
        let crt = ResidentCrtPipelines::create(&ctx.device)?;
        let palette = ResidentPalettePipelines::create(&ctx.device)?;
        let palette_guided = ResidentPaletteGuidedPipelines::create(&ctx.device)?;
        let composite = ResidentCompositePipelines::create(&ctx.device)?;
        let (tx, rx) = bounded(QUEUE_DEPTH);
        let join = thread::Builder::new()
            .name("gpu-executor".into())
            .spawn(move || {
                executor_loop(
                    ctx,
                    cache,
                    bayer,
                    halftone,
                    crt,
                    palette,
                    palette_guided,
                    composite,
                    rx,
                )
            })
            .map_err(|e| GpuError::Device(format!("spawn gpu-executor: {e}")))?;
        Ok(Self {
            tx,
            join: Some(join),
        })
    }

    pub fn submit_frame(&self, job: GpuFrameJob) {
        let _ = self.tx.try_send(ExecutorMsg::Frame(job));
    }

    /// Submit and block until the executor finishes this frame (tests / diagnostics).
    pub fn submit_frame_blocking(&self, job: GpuFrameJob) -> Result<(), GpuError> {
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        self.tx
            .send(ExecutorMsg::FrameWithAck { job, done_tx })
            .map_err(|_| GpuError::Device("gpu executor channel closed".into()))?;
        done_rx
            .recv()
            .map_err(|_| GpuError::Device("gpu executor stopped before ack".into()))?
    }

    pub fn submit_composite(&self, job: GpuCompositeFrameJob) {
        let _ = self.tx.try_send(ExecutorMsg::Composite(job));
    }

    pub fn submit_composite_blocking(&self, job: GpuCompositeFrameJob) -> Result<(), GpuError> {
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        self.tx
            .send(ExecutorMsg::CompositeWithAck { job, done_tx })
            .map_err(|_| GpuError::Device("gpu executor channel closed".into()))?;
        done_rx
            .recv()
            .map_err(|_| GpuError::Device("gpu executor stopped before ack".into()))?
    }

    pub fn shutdown(&mut self) {
        let _ = self.tx.send(ExecutorMsg::Shutdown);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

impl Drop for GpuExecutor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn executor_loop(
    ctx: Arc<GpuContext>,
    cache: Arc<GpuTileCache>,
    bayer: ResidentBayerPipelines,
    halftone: ResidentHalftonePipelines,
    crt: ResidentCrtPipelines,
    palette: ResidentPalettePipelines,
    palette_guided: ResidentPaletteGuidedPipelines,
    composite: ResidentCompositePipelines,
    rx: Receiver<ExecutorMsg>,
) {
    let gather = match ResidentGatherPipelines::create(&ctx.device) {
        Ok(g) => Some(g),
        Err(e) => {
            log::warn!("engine-gpu resident gather pipelines unavailable: {e}");
            None
        }
    };
    let readback = ReadbackRing::for_tile_core(&ctx.device);

    while let Ok(msg) = rx.recv() {
        match msg {
            ExecutorMsg::Shutdown => break,
            ExecutorMsg::Frame(job) => {
                if let Err(e) = run_frame(
                    &ctx,
                    &cache,
                    &bayer,
                    &halftone,
                    &crt,
                    &palette,
                    &palette_guided,
                    gather.as_ref(),
                    &readback,
                    &job,
                ) {
                    log::warn!("engine-gpu resident frame failed: {e}");
                }
            }
            ExecutorMsg::FrameWithAck { job, done_tx } => {
                let res = run_frame(
                    &ctx,
                    &cache,
                    &bayer,
                    &halftone,
                    &crt,
                    &palette,
                    &palette_guided,
                    gather.as_ref(),
                    &readback,
                    &job,
                );
                if let Err(ref e) = res {
                    log::warn!("engine-gpu resident frame failed: {e}");
                }
                let _ = done_tx.send(res);
            }
            ExecutorMsg::Composite(job) => {
                if let Err(e) = run_composite_frame(&ctx, &cache, &composite, &job) {
                    log::warn!("engine-gpu resident composite failed: {e}");
                }
            }
            ExecutorMsg::CompositeWithAck { job, done_tx } => {
                let res = run_composite_frame(&ctx, &cache, &composite, &job);
                if let Err(ref e) = res {
                    log::warn!("engine-gpu resident composite failed: {e}");
                }
                let _ = done_tx.send(res);
            }
        }
    }
}

fn run_frame(
    ctx: &GpuContext,
    cache: &GpuTileCache,
    bayer: &ResidentBayerPipelines,
    halftone: &ResidentHalftonePipelines,
    crt: &ResidentCrtPipelines,
    palette: &ResidentPalettePipelines,
    palette_guided: &ResidentPaletteGuidedPipelines,
    gather: Option<&ResidentGatherPipelines>,
    readback: &ReadbackRing,
    job: &GpuFrameJob,
) -> Result<(), GpuError> {
    if job.tiles.is_empty() {
        return Ok(());
    }

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu-resident-frame"),
        });

    bayer.begin_frame();

    let mut in_flight = Vec::with_capacity(job.tiles.len());

    for (batch_i, work) in job.tiles.iter().enumerate() {
        let slot = match cache.get_slot(&work.key, work.generation) {
            Some(s) => s,
            None => cache.promote(ctx, work.key, &work.pixels, work.generation)?,
        };
        in_flight.push(slot);

        let scratch_layer = batch_i as u32;

        for node in &job.graph.nodes {
            match node {
                GraphNode::Gpu(pass) => {
                    if let Some(bayer_params) = pass.bayer {
                        if matches!(
                            pass.pipeline,
                            GpuPipelineKey::Bayer2 | GpuPipelineKey::Bayer4 | GpuPipelineKey::Bayer8
                        ) {
                            bayer.encode_bayer_pass(
                                &ctx.device,
                                &ctx.queue,
                                &mut encoder,
                                cache.resident_texture(),
                                slot.index,
                                cache.scratch_a(),
                                scratch_layer,
                                work.coord.x,
                                work.coord.y,
                                bayer_params,
                            );
                            ResidentBayerPipelines::copy_layer(
                                &mut encoder,
                                cache.scratch_a(),
                                scratch_layer,
                                cache.resident_texture(),
                                slot.index,
                            );
                        }
                    }
                    if let Some(ht) = pass.halftone {
                        if pass.pipeline == GpuPipelineKey::Halftone {
                            halftone.encode_halftone_pass(
                                &ctx.device,
                                &mut encoder,
                                cache.resident_texture(),
                                slot.index,
                                cache.scratch_a(),
                                scratch_layer,
                                work.coord.x,
                                work.coord.y,
                                ht,
                            );
                            ResidentBayerPipelines::copy_layer(
                                &mut encoder,
                                cache.scratch_a(),
                                scratch_layer,
                                cache.resident_texture(),
                                slot.index,
                            );
                        }
                    }
                    if let Some(crt_params) = pass.crt {
                        if pass.pipeline == GpuPipelineKey::Crt {
                            crt.encode_crt_pass(
                                &ctx.device,
                                &mut encoder,
                                cache.resident_texture(),
                                slot.index,
                                cache.scratch_a(),
                                scratch_layer,
                                work.coord.x,
                                work.coord.y,
                                crt_params,
                            );
                            ResidentBayerPipelines::copy_layer(
                                &mut encoder,
                                cache.scratch_a(),
                                scratch_layer,
                                cache.resident_texture(),
                                slot.index,
                            );
                        }
                    }
                    if let Some(pq) = &pass.palette_quantize {
                        if pass.pipeline == GpuPipelineKey::PaletteQuantize {
                            palette.encode_palette_quantize_pass(
                                &ctx.device,
                                &mut encoder,
                                cache.resident_texture(),
                                slot.index,
                                cache.scratch_a(),
                                scratch_layer,
                                work.coord.x,
                                work.coord.y,
                                pq,
                            );
                            ResidentBayerPipelines::copy_layer(
                                &mut encoder,
                                cache.scratch_a(),
                                scratch_layer,
                                cache.resident_texture(),
                                slot.index,
                            );
                        }
                    }
                    if let Some(guided) = &pass.palette_guided {
                        if pass.pipeline == GpuPipelineKey::PaletteGuided {
                            palette_guided.encode_guided_pass(
                                &ctx.device,
                                &mut encoder,
                                cache.resident_texture(),
                                slot.index,
                                cache.scratch_a(),
                                scratch_layer,
                                work.coord.x,
                                work.coord.y,
                                guided,
                            );
                            ResidentBayerPipelines::copy_layer(
                                &mut encoder,
                                cache.scratch_a(),
                                scratch_layer,
                                cache.resident_texture(),
                                slot.index,
                            );
                        }
                    }
                    if let Some(mixed) = &pass.palette_mixed {
                        if pass.pipeline == GpuPipelineKey::PaletteMixed {
                            // Pass 1: Guided → scratch A
                            palette_guided.encode_guided_pass(
                                &ctx.device,
                                &mut encoder,
                                cache.resident_texture(),
                                slot.index,
                                cache.scratch_a(),
                                scratch_layer,
                                work.coord.x,
                                work.coord.y,
                                &mixed.guided,
                            );
                            // Pass 2: ordered snap A → B
                            palette_guided.encode_ordered_snap_pass(
                                &ctx.device,
                                &mut encoder,
                                cache.scratch_a(),
                                scratch_layer,
                                cache.scratch_b(),
                                scratch_layer,
                                work.coord.x,
                                work.coord.y,
                                mixed,
                            );
                            ResidentBayerPipelines::copy_layer(
                                &mut encoder,
                                cache.scratch_b(),
                                scratch_layer,
                                cache.resident_texture(),
                                slot.index,
                            );
                        }
                    }
                }
                GraphNode::CpuCheckpoint(_) => {
                    return Err(GpuError::Device(
                        "CpuCheckpoint in GPU-only frame path".into(),
                    ));
                }
            }
        }
    }

    // Phase 1: gather → staging copy into readback ring (MAP_READ buffers cannot be STORAGE).
    if let Some(gather) = gather {
        use crate::resident::TILE_CORE_RGBA8_BYTES;

        for slot in &in_flight {
            let gather_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gather-rgba8-out"),
                size: TILE_CORE_RGBA8_BYTES,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            gather.encode_gather(
                &ctx.device,
                &mut encoder,
                cache.resident_texture(),
                slot.index,
                &gather_buf,
            );
            let staging = readback.next_buffer();
            encoder.copy_buffer_to_buffer(
                &gather_buf,
                0,
                staging,
                0,
                TILE_CORE_RGBA8_BYTES,
            );
        }
    }

    cache.mark_in_flight(&in_flight);
    ctx.queue.submit(Some(encoder.finish()));
    ctx.device.poll(wgpu::Maintain::Wait);
    cache.clear_in_flight();
    Ok(())
}

fn run_composite_frame(
    ctx: &GpuContext,
    cache: &GpuTileCache,
    composite: &ResidentCompositePipelines,
    job: &GpuCompositeFrameJob,
) -> Result<(), GpuError> {
    if job.tiles.is_empty() {
        return Ok(());
    }

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu-resident-composite-frame"),
        });

    composite.begin_frame();

    let mut in_flight = Vec::new();
    let transparent = Arc::new(PixelTile::new());

    for (batch_i, work) in job.tiles.iter().enumerate() {
        let scratch_layer = batch_i as u32;
        let mut src_layers = Vec::with_capacity(work.layers.len());
        for layer in &work.layers {
            let slot = match cache.get_slot(&layer.processed_key, work.generation) {
                Some(s) => s,
                None => {
                    let pixels = layer.pixels.as_ref().ok_or_else(|| {
                        GpuError::Device(format!(
                            "composite: missing resident Processed {:?}",
                            layer.processed_key
                        ))
                    })?;
                    cache.promote(ctx, layer.processed_key, pixels, work.generation)?
                }
            };
            in_flight.push(slot);
            src_layers.push((slot.index, layer.blend_mode, layer.opacity));
        }

        let out_slot = match cache.get_slot(&work.composite_key, work.generation) {
            Some(s) => s,
            None => cache.promote(
                ctx,
                work.composite_key,
                &transparent,
                work.generation,
            )?,
        };
        in_flight.push(out_slot);

        // Write fused stack into scratch (separate from resident) to avoid R/W hazard.
        composite.encode_stack_pass(
            &ctx.device,
            &ctx.queue,
            &mut encoder,
            cache.resident_texture(),
            cache.scratch_a(),
            scratch_layer,
            &src_layers,
        )?;
        ResidentBayerPipelines::copy_layer(
            &mut encoder,
            cache.scratch_a(),
            scratch_layer,
            cache.resident_texture(),
            out_slot.index,
        );
    }

    cache.mark_in_flight(&in_flight);
    ctx.queue.submit(Some(encoder.finish()));
    ctx.device.poll(wgpu::Maintain::Wait);
    cache.clear_in_flight();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executor_spawns_and_shuts_down() {
        let Some(ctx) = GpuContext::try_new_blocking().map(Arc::new) else {
            return;
        };
        let cache = Arc::new(GpuTileCache::with_defaults(&ctx.device));
        let mut ex = GpuExecutor::spawn(Arc::clone(&ctx), cache).expect("spawn");
        ex.shutdown();
    }
}
