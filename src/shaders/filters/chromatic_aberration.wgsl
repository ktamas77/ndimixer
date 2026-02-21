// Chromatic aberration — RGB channel split.
//
// Params (alphabetical order):
//   params[0] = amount  (0.0 to 20.0, default 2.0 — pixel offset)
//   params[1] = angle   (0.0 to 6.28, default 0.0 — radians, direction of split)

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

    let amount = uniforms.params[0].x;
    let angle = uniforms.params[0].y;

    let offset = vec2f(cos(angle), sin(angle)) * amount;
    let offset_i = vec2i(i32(offset.x), i32(offset.y));

    // Clamp sample positions to texture bounds
    let dims = vec2i(i32(w), i32(h));
    let pos_r = clamp(pos + offset_i, vec2i(0), dims - vec2i(1));
    let pos_b = clamp(pos - offset_i, vec2i(0), dims - vec2i(1));

    let center = textureLoad(input_tex, pos, 0);
    let r_sample = textureLoad(input_tex, pos_r, 0);
    let b_sample = textureLoad(input_tex, pos_b, 0);

    let result = vec4f(r_sample.r, center.g, b_sample.b, center.a);

    textureStore(output_tex, pos, result);
}
