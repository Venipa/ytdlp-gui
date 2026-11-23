// Reads custom shader instances from a storage buffer, the transport every native backend uses.

@group(1) @binding(0) var<storage, read> gpui_shader_instances: array<GpuiShaderInstance>;

fn gpui_load_shader_instance(index: u32) -> GpuiShaderInstance {
    return gpui_shader_instances[index];
}
