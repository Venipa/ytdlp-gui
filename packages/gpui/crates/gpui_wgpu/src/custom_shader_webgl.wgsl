// Reads custom shader instances from the instance texture on WebGL2, which has no storage
// buffers. An instance is 52 words, so it spans exactly 13 texels and starts on a texel.

@group(1) @binding(0) var gpui_instance_texels: texture_2d<u32>;

fn gpui_instance_texel(index: u32) -> vec4<u32> {
    let width = textureDimensions(gpui_instance_texels).x;
    return textureLoad(gpui_instance_texels, vec2<i32>(i32(index % width), i32(index / width)), 0);
}

fn gpui_load_shader_instance(index: u32) -> GpuiShaderInstance {
    let base = index * 13u;
    let header = gpui_instance_texel(base);
    let bounds = bitcast<vec4<f32>>(gpui_instance_texel(base + 1u));
    let mask = bitcast<vec4<f32>>(gpui_instance_texel(base + 2u));
    let tail = gpui_instance_texel(base + 4u);

    var instance: GpuiShaderInstance;
    instance.order = header.x;
    instance.shader = header.y;
    instance.opacity = bitcast<f32>(header.z);
    instance.time = bitcast<f32>(header.w);
    instance.bounds_origin = bounds.xy;
    instance.bounds_size = bounds.zw;
    instance.mask_origin = mask.xy;
    instance.mask_size = mask.zw;
    instance.corner_radii = bitcast<vec4<f32>>(gpui_instance_texel(base + 3u));
    instance.scale_factor = bitcast<f32>(tail.x);
    for (var i = 0u; i < 4u; i++) {
        instance.params[i] = bitcast<vec4<f32>>(gpui_instance_texel(base + 5u + i));
        instance.colors[i] = bitcast<vec4<f32>>(gpui_instance_texel(base + 9u + i));
    }
    return instance;
}
