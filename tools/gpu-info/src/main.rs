// Port-only: prints the adapter that mln_webgpu's HeadlessBackend requests
// (high performance, backends from WGPU_BACKEND), its features, limits and
// downlevel capabilities.

fn main() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or_default(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    })) {
        Ok(adapter) => adapter,
        Err(e) => {
            eprintln!("no adapter: {e}");
            std::process::exit(1);
        }
    };
    println!("{:#?}", adapter.get_info());
    println!("{:#?}", adapter.features());
    println!("{:#?}", adapter.limits());
    println!("{:#?}", adapter.get_downlevel_capabilities());
}
