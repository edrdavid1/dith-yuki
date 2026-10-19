//! Opt-in GPU dispatch breakdown when `DITHER_PROFILE=1` (`.local-doc/time-test-2.md`).
//!
//! Production path: every entry returns immediately when profiling is off.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Instant;

/// Capacity: 2 timestamps × dispatches. Spec default when unknown.
pub const MAX_DISPATCHES_PER_APPLY: u32 = 1024;

static PROFILE_ENV: AtomicU8 = AtomicU8::new(0); // 0=unknown, 1=off, 2=on
static TS_AVAILABLE: AtomicBool = AtomicBool::new(false);
static TS_LOGGED_UNAVAILABLE: AtomicBool = AtomicBool::new(false);
static FEATURES_LOGGED: AtomicBool = AtomicBool::new(false);

fn profile_on() -> bool {
    match PROFILE_ENV.load(Ordering::Relaxed) {
        2 => true,
        1 => false,
        _ => {
            let on = std::env::var("DITHER_PROFILE")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            PROFILE_ENV.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
    }
}

/// Features to request at blocked_fs device create when profiling is on.
pub fn requested_timestamp_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    if !profile_on() {
        return wgpu::Features::empty();
    }
    let avail = adapter.features();
    let mut f = wgpu::Features::empty();
    if avail.contains(wgpu::Features::TIMESTAMP_QUERY) {
        f |= wgpu::Features::TIMESTAMP_QUERY;
    }
    // Pass begin/end timestamps use TIMESTAMP_QUERY; INSIDE_PASSES is not required
    // for ComputePassTimestampWrites on wgpu 24, but request if present.
    if avail.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES) {
        f |= wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
    }
    if avail.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS) {
        f |= wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    }
    f
}

/// Record whether the created device actually has TIMESTAMP_QUERY.
pub fn set_device_timestamp_available(granted: bool, adapter_had: bool) {
    TS_AVAILABLE.store(granted, Ordering::Relaxed);
    // Log only when profiling — avoid production Info noise on every device create.
    if profile_on() && !FEATURES_LOGGED.swap(true, Ordering::Relaxed) {
        log::info!(
            target: "diag_gpu_timestamp",
            "timestamp_query adapter_advertised={} device_granted={} dither_profile=true",
            adapter_had,
            granted
        );
        if !granted {
            log::info!(
                target: "diag_gpu_timestamp",
                "timestamp_query=unsupported"
            );
        }
    }
}

pub fn timestamp_query_available() -> bool {
    TS_AVAILABLE.load(Ordering::Relaxed)
}

/// Host + optional GPU query timing for one apply's dispatch loop.
pub struct DispatchTimestampDiag {
    mode: &'static str,
    width: u32,
    height: u32,
    query_set: Option<wgpu::QuerySet>,
    resolve_buf: Option<wgpu::Buffer>,
    next_query: u32,
    dispatches: u32,
    submit_ms: Vec<f64>,
    last_submit_end: Option<Instant>,
    poll_ms: f64,
    /// True when each strip polls before the next submit (not one final poll).
    poll_per_dispatch: bool,
    gpu_kernel_ms: Vec<f64>,
    kernel_status: &'static str,
}

impl DispatchTimestampDiag {
    pub fn begin(
        device: &wgpu::Device,
        mode: &'static str,
        width: u32,
        height: u32,
    ) -> Option<Self> {
        if !profile_on() {
            return None;
        }
        let mut query_set = None;
        let mut resolve_buf = None;
        let mut kernel_status = "unavailable";
        if timestamp_query_available() {
            let count = (2 * MAX_DISPATCHES_PER_APPLY).min(wgpu::QUERY_SET_MAX_QUERIES);
            let qs = device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("dither_ts_qs"),
                ty: wgpu::QueryType::Timestamp,
                count,
            });
            let bytes = (count as u64) * 8;
            let resolve = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("dither_ts_resolve"),
                size: bytes,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            query_set = Some(qs);
            resolve_buf = Some(resolve);
            kernel_status = "pending";
        } else if !TS_LOGGED_UNAVAILABLE.swap(true, Ordering::Relaxed) {
            log::info!(
                target: "diag_gpu_timestamp",
                "timestamp_query=unsupported (wall-clock submit/poll only)"
            );
        }
        Some(Self {
            mode,
            width,
            height,
            query_set,
            resolve_buf,
            next_query: 0,
            dispatches: 0,
            submit_ms: Vec::new(),
            last_submit_end: None,
            poll_ms: 0.0,
            poll_per_dispatch: false,
            gpu_kernel_ms: Vec::new(),
            kernel_status,
        })
    }

    /// Encode one compute dispatch + submit; returns host submit_ms contribution.
    pub fn dispatch_submit(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &wgpu::ComputePipeline,
        bind_group: &wgpu::BindGroup,
        groups_x: u32,
        groups_y: u32,
        label: &str,
    ) {
        if self.dispatches >= MAX_DISPATCHES_PER_APPLY {
            // Cap: still submit without more query slots (behavior unchanged aside from missing TS).
            let t0 = Instant::now();
            submit_plain(
                device, queue, pipeline, bind_group, groups_x, groups_y, label,
            );
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            self.submit_ms.push(ms);
            self.last_submit_end = Some(Instant::now());
            self.dispatches += 1;
            return;
        }

        let t_begin = Instant::now();
        let mut enc =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });

        let ts_writes = if let Some(ref qs) = self.query_set {
            let begin = self.next_query;
            let end = self.next_query + 1;
            self.next_query += 2;
            Some(wgpu::ComputePassTimestampWrites {
                query_set: qs,
                beginning_of_pass_write_index: Some(begin),
                end_of_pass_write_index: Some(end),
            })
        } else {
            None
        };

        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(label),
                timestamp_writes: ts_writes,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }
        queue.submit(Some(enc.finish()));
        let t_end = Instant::now();
        self.submit_ms
            .push(t_begin.elapsed().as_secs_f64() * 1000.0);
        self.last_submit_end = Some(t_end);
        self.dispatches += 1;
    }

    /// After `device.poll(Maintain::Wait)` for this apply (or strip).
    pub fn note_poll_end(&mut self, per_dispatch: bool) {
        self.poll_per_dispatch = self.poll_per_dispatch || per_dispatch;
        if let Some(last) = self.last_submit_end {
            let ms = last.elapsed().as_secs_f64() * 1000.0;
            if per_dispatch {
                self.poll_ms += ms;
            } else {
                self.poll_ms = ms;
            }
        }
    }

    /// Resolve GPU timestamps (if any) and emit the JSON diag line.
    pub fn finish(
        mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        strips: u32,
        dispatch_wall_ms: f64,
    ) {
        if let (Some(qs), Some(resolve)) = (self.query_set.take(), self.resolve_buf.take()) {
            let used = self.next_query;
            if used > 0 {
                match read_timestamps(device, queue, &qs, &resolve, used) {
                    Ok(ticks) => {
                        let period = queue.get_timestamp_period() as f64;
                        // pairs (begin, end)
                        let mut i = 0usize;
                        while i + 1 < ticks.len() {
                            let dt_ns = (ticks[i + 1].wrapping_sub(ticks[i]) as f64) * period;
                            self.gpu_kernel_ms.push(dt_ns / 1_000_000.0);
                            i += 2;
                        }
                        self.kernel_status = "available";
                    }
                    Err(reason) => {
                        self.kernel_status = "unavailable";
                        log::info!(
                            target: "diag_gpu_timestamp",
                            "kernel_ms=unavailable reason={reason}"
                        );
                    }
                }
            } else {
                self.kernel_status = "unavailable";
            }
        } else if self.kernel_status == "pending" {
            self.kernel_status = "unavailable";
        } else if !timestamp_query_available() {
            self.kernel_status = "unsupported";
        }

        let submit_sum_ms: f64 = self.submit_ms.iter().sum();
        let kernel_sum_ms: f64 = self.gpu_kernel_ms.iter().sum();
        let kernel_max_ms = self.gpu_kernel_ms.iter().copied().fold(0.0_f64, f64::max);

        let ts_label = match self.kernel_status {
            "available" => "available",
            "unsupported" => "unsupported",
            _ => "unavailable",
        };

        let line = serde_json::json!({
            "mode": self.mode,
            "size": format!("{}x{}", self.width, self.height),
            "dispatches": self.dispatches,
            "submit_sum_ms": round3(submit_sum_ms),
            "poll_ms": round3(self.poll_ms),
            "dispatch_wall_ms": round3(dispatch_wall_ms),
            "kernel_sum_ms": if self.kernel_status == "available" {
                serde_json::Value::from(round3(kernel_sum_ms))
            } else {
                serde_json::Value::Null
            },
            "kernel_max_ms": if self.kernel_status == "available" {
                serde_json::Value::from(round3(kernel_max_ms))
            } else {
                serde_json::Value::Null
            },
            "kernel_ms": self.kernel_status,
            "timestamp_query": ts_label,
            "strips": strips,
            "poll_per_dispatch": self.poll_per_dispatch,
        });
        log::info!(target: "diag_gpu_timestamp", "[diag/gpu_timestamp] {line}");
        // Also stderr so --nocapture harness captures without RUST_LOG.
        eprintln!("[diag/gpu_timestamp] {line}");
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn submit_plain(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    groups_x: u32,
    groups_y: u32,
    label: &str,
) {
    let mut enc =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) });
    {
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(label),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.dispatch_workgroups(groups_x, groups_y, 1);
    }
    queue.submit(Some(enc.finish()));
}

fn read_timestamps(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    query_set: &wgpu::QuerySet,
    resolve_buf: &wgpu::Buffer,
    count: u32,
) -> Result<Vec<u64>, String> {
    let bytes = (count as u64) * 8;
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("dither_ts_resolve"),
    });
    enc.resolve_query_set(query_set, 0..count, resolve_buf, 0);
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("dither_ts_staging"),
        size: bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    enc.copy_buffer_to_buffer(resolve_buf, 0, &staging, 0, bytes);
    queue.submit(Some(enc.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::Maintain::Wait);
    rx.recv()
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("ts map: {e}"))?;
    let data = slice.get_mapped_range();
    let ticks: Vec<u64> = bytemuck::cast_slice(&data).to_vec();
    drop(data);
    staging.unmap();
    Ok(ticks)
}
