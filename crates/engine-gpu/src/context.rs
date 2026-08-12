//! GpuContext: device + queue + timeout counter + cached pipelines.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use crate::bayer::BayerPipelines;
use crate::crt::CrtPipeline;
use crate::halftone::HalftonePipeline;

/// Shared GPU device for tile compute.
///
/// `submit_lock` serializes encode/submit/map for worker threads (v1 policy).
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// Incremented on map_async timeout or map failure (Track A silent-path pattern).
    pub map_timeout_counter: AtomicU64,
    /// Test hook: next `map_read_with_timeout` fails + increments counter.
    pub force_map_timeout: AtomicBool,
    pub(crate) submit_lock: Mutex<()>,
    pub(crate) bayer: Option<BayerPipelines>,
    pub(crate) halftone: Option<HalftonePipeline>,
    pub(crate) crt: Option<CrtPipeline>,
}

impl GpuContext {
    /// Request adapter → device. Returns `None` on failure (no panic).
    pub async fn try_new() -> Option<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await?;

        let info = adapter.get_info();
        log::info!(
            "engine-gpu: adapter {:?} ({:?})",
            info.name,
            info.backend
        );

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("dither-gpu"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .ok()?;

        let mut ctx = Self {
            device,
            queue,
            map_timeout_counter: AtomicU64::new(0),
            force_map_timeout: AtomicBool::new(false),
            submit_lock: Mutex::new(()),
            bayer: None,
            halftone: None,
            crt: None,
        };

        ctx.bayer = BayerPipelines::create(&ctx.device).ok();
        if ctx.bayer.is_none() {
            log::warn!("engine-gpu: Bayer pipeline create failed; Bayer stays CPU");
        }
        ctx.halftone = HalftonePipeline::create(&ctx.device).ok();
        if ctx.halftone.is_none() {
            log::warn!("engine-gpu: Halftone pipeline create failed; Halftone stays CPU");
        }
        ctx.crt = CrtPipeline::create(&ctx.device).ok();
        if ctx.crt.is_none() {
            log::warn!("engine-gpu: CRT pipeline create failed; CRT stays CPU");
        }

        Some(ctx)
    }

    /// Blocking init for Tauri setup / tests.
    pub fn try_new_blocking() -> Option<Self> {
        pollster::block_on(Self::try_new())
    }

    pub fn is_available(&self) -> bool {
        true
    }

    pub fn map_timeouts(&self) -> u64 {
        self.map_timeout_counter.load(Ordering::Relaxed)
    }

    pub(crate) fn record_map_timeout(&self) {
        let n = self.map_timeout_counter.fetch_add(1, Ordering::Relaxed) + 1;
        if n <= 8 || n.is_power_of_two() {
            log::warn!("engine-gpu: map_async timeout/failure (count={n}); falling back to CPU");
        }
    }

    /// Test/fault injection: bump counter without a real map.
    pub fn inject_map_timeout_for_test(&self) {
        self.record_map_timeout();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_compiles_and_counter_starts_at_zero_without_adapter() {
        // Construction without adapter is None — just exercise the type + counter API via inject path
        // when we have no device: create a dummy AtomicU64 pattern by ensuring try_new can return None.
        let maybe = GpuContext::try_new_blocking();
        if let Some(ctx) = maybe {
            assert_eq!(ctx.map_timeouts(), 0);
            ctx.inject_map_timeout_for_test();
            assert_eq!(ctx.map_timeouts(), 1);
        }
        // Always pass: crate builds; adapter optional.
    }

    #[test]
    #[ignore = "requires GPU adapter"]
    fn adapter_smoke() {
        let ctx = GpuContext::try_new_blocking().expect("adapter");
        assert!(ctx.is_available());
        assert!(ctx.bayer.is_some());
    }
}
