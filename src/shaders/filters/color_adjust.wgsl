// Color adjustment filter — brightness, contrast, saturation.
//
// Params (alphabetical order):
//   params[0] = brightness  (-1.0 to 1.0, default 0.0)
//   params[1] = contrast    (0.0 to 3.0, default 1.0)
//   params[2] = saturation  (0.0 to 3.0, default 1.0)

struct FilterUniforms {
    time: f32,
    width: f32,
    height: f32,
    param_count: f32,
    params: array<vec4f, 4>,
}

@group(0) @binding(0) var input_tex: texture_2d<f32>;
@group(0) @binding(1) var output_tex: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> uniforms: FilterUniforms;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3u) {
    let w = u32(uniforms.width);
    let h = u32(uniforms.height);
    if gid.x >= w || gid.y >= h {
        return;
    }

    let pos = vec2i(vec2u(gid.xy));
    let color = textureLoad(input_tex, pos, 0);

    let brightness = uniforms.params[0].x;
    let contrast = uniforms.params[0].y;
    let saturation = uniforms.params[0].z;

    // Apply brightness
    var rgb = color.rgb + vec3f(brightness);

    // Apply contrast (around 0.5 midpoint)
    rgb = (rgb - vec3f(0.5)) * contrast + vec3f(0.5);

    // Apply saturation
    let luma = dot(rgb, vec3f(0.2126, 0.7152, 0.0722));
    rgb = mix(vec3f(luma), rgb, saturation);

    // Clamp to valid range
    rgb = clamp(rgb, vec3f(0.0), vec3f(1.0));

    textureStore(output_tex, pos, vec4f(rgb, color.a));
}
