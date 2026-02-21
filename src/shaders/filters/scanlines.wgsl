// CRT scanline effect with optional scrolling.
//
// Params (alphabetical order):
//   params[0] = intensity  (0.0 to 1.0, default 0.3 — darkness of scanlines)
//   params[1] = scroll     (0.0 to 10.0, default 0.0 — scroll speed in lines/sec)
//   params[2] = spacing    (1.0 to 20.0, default 2.0 — pixels between scanlines)

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

    let intensity = uniforms.params[0].x;
    let scroll = uniforms.params[0].y;
    let spacing = max(uniforms.params[0].z, 1.0);

    // Scrolling offset based on time
    let y_offset = uniforms.time * scroll * spacing;
    let y = f32(gid.y) + y_offset;

    // Scanline pattern: darken every Nth line
    let line_pos = (y % spacing) / spacing;
    let scanline = 1.0 - intensity * step(0.5, line_pos);

    let rgb = color.rgb * scanline;

    textureStore(output_tex, pos, vec4f(rgb, color.a));
}
