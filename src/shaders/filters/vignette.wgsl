// Vignette — edge darkening effect.
//
// Params (alphabetical order):
//   params[0] = radius   (0.0 to 2.0, default 0.8 — inner radius before darkening starts)
//   params[1] = softness (0.0 to 2.0, default 0.3 — transition width)

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

    let radius = uniforms.params[0].x;
    let softness = max(uniforms.params[0].y, 0.001);

    // Normalized coordinates centered at (0.5, 0.5)
    let uv = vec2f(f32(gid.x) / uniforms.width, f32(gid.y) / uniforms.height);
    let center = vec2f(0.5, 0.5);
    let dist = distance(uv, center) * 2.0; // 0.0 at center, ~1.41 at corners

    // Smooth falloff from radius to radius+softness
    let vignette = 1.0 - smoothstep(radius, radius + softness, dist);

    let rgb = color.rgb * vignette;

    textureStore(output_tex, pos, vec4f(rgb, color.a));
}
