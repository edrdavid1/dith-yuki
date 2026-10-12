fn main() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("adapter");
    let l = adapter.limits();
    let info = adapter.get_info();
    println!("adapter={} {:?}", info.name, info.backend);
    println!("max_compute_workgroup_storage_size={}", l.max_compute_workgroup_storage_size);
    println!("max_compute_invocations_per_workgroup={}", l.max_compute_invocations_per_workgroup);
    println!("max_compute_workgroup_size_x={}", l.max_compute_workgroup_size_x);
    println!("max_compute_workgroup_size_y={}", l.max_compute_workgroup_size_y);
}
