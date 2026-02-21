// Drop shadow — renders a shadow behind opaque/semi-transparent content.
//
// Params (alphabetical order):
//   params[0] = angle     (0.0 to 6.28, default 0.785 — direction in radians, 0.785 ≈ 45°)
//   params[1] = distance  (0.0 to 50.0, default 5.0 — shadow offset in pixels)
//   params[2] = opacity   (0.0 to 1.0, default 0.5 — shadow darkness)
//   params[3] = softness  (0.0 to 20.0, default 3.0 — blur radius in pixels)

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
    let dims = vec2i(i32(w), i32(h));

    let angle = uniforms.params[0].x;
    let distance = uniforms.params[0].y;
    let opacity = uniforms.params[0].z;
    let softness = max(uniforms.params[0].w, 0.0);

    // Shadow offset direction
    let offset = vec2f(cos(angle), sin(angle)) * distance;

    // Sample alpha in a grid around the shadow-offset position to create soft edges.
    // The position we sample from is shifted *opposite* to the shadow direction:
    // "where would a foreground pixel need to be to cast shadow here?"
    let shadow_sample_center = vec2f(f32(gid.x) - offset.x, f32(gid.y) - offset.y);

    var shadow_alpha = 0.0;

    if softness < 0.5 {
        // Hard shadow — single sample
        let sp = clamp(vec2i(i32(shadow_sample_center.x + 0.5), i32(shadow_sample_center.y + 0.5)), vec2i(0), dims - vec2i(1));
        shadow_alpha = textureLoad(input_tex, sp, 0).a;
    } else {
        // Soft shadow — box-blur kernel over the source alpha
        let radius = i32(ceil(softness));
        let r = min(radius, 10); // cap at 10 to keep performance reasonable
        var total = 0.0;
        var weight_sum = 0.0;

        for (var dy = -r; dy <= r; dy++) {
            for (var dx = -r; dx <= r; dx++) {
                let d = length(vec2f(f32(dx), f32(dy)));
                if d > softness {
                    continue;
                }
                // Gaussian-ish weight falloff
                let w_val = 1.0 - (d / (softness + 0.001));
                let sp = vec2i(
                    i32(shadow_sample_center.x + f32(dx) + 0.5),
                    i32(shadow_sample_center.y + f32(dy) + 0.5)
                );
                let clamped = clamp(sp, vec2i(0), dims - vec2i(1));
                total += textureLoad(input_tex, clamped, 0).a * w_val;
                weight_sum += w_val;
            }
        }

        if weight_sum > 0.0 {
            shadow_alpha = total / weight_sum;
        }
    }

    let foreground = textureLoad(input_tex, pos, 0);

    // Shadow color (black) with computed alpha
    let sa = shadow_alpha * opacity;

    // Composite: shadow behind foreground (premultiplied-style)
    // shadow first, then foreground "over" shadow
    let inv_fa = 1.0 - foreground.a;
    let out_a = foreground.a + sa * inv_fa;

    var out_rgb = foreground.rgb;
    if out_a > 0.0 {
        // Shadow contributes black (0,0,0) so only foreground adds color
        out_rgb = foreground.rgb * foreground.a / out_a;
    }

    textureStore(output_tex, pos, vec4f(out_rgb, out_a));
}
