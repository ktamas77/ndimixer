// GPU compute compositor for ndimixer.
//
// Two entry points:
//   clear — fill output texture with opaque black
//   blend — Porter-Duff "source over" with per-layer opacity
//
// Uses compute dispatches only (no render pass) to avoid the
// Metal backend memory leak in wgpu renderCommandEncoder.

struct Params {
    opacity: f32,
    width: u32,
    height: u32,
    _pad: u32,
}

// ---- Clear pipeline ----

@group(0) @binding(0) var clear_dst: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(1) var<uniform> clear_params: Params;

@compute @workgroup_size(16, 16)
fn clear(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= clear_params.width || gid.y >= clear_params.height {
        return;
    }
    textureStore(clear_dst, vec2i(vec2u(gid.xy)), vec4f(0.0, 0.0, 0.0, 1.0));
}

// ---- Blend pipeline ----

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var layer: texture_2d<f32>;
@group(0) @binding(2) var dst: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(3) var<uniform> blend_params: Params;

@compute @workgroup_size(16, 16)
fn blend(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= blend_params.width || gid.y >= blend_params.height {
        return;
    }

    let pos = vec2i(vec2u(gid.xy));
    let d = textureLoad(src, pos, 0);
    let s = textureLoad(layer, pos, 0);

    let sa = s.a * blend_params.opacity;

    // Fully transparent — pass through destination
    if sa <= 0.0 {
        textureStore(dst, pos, d);
        return;
    }

    // Fully opaque — replace destination
    if sa >= 1.0 {
        textureStore(dst, pos, vec4f(s.rgb, 1.0));
        return;
    }

    // Porter-Duff "source over":
    //   out_a   = sa + da * (1 - sa)
    //   out_rgb = (src * sa + dst * da * (1 - sa)) / out_a
    let inv_sa = 1.0 - sa;
    let out_a = sa + d.a * inv_sa;

    if out_a <= 0.0 {
        textureStore(dst, pos, vec4f(0.0, 0.0, 0.0, 0.0));
        return;
    }

    let da_inv = d.a * inv_sa;
    let out_rgb = (s.rgb * sa + d.rgb * da_inv) / out_a;

    textureStore(dst, pos, vec4f(out_rgb, out_a));
}
