use image::{ImageBuffer, Rgba, RgbaImage};

pub struct Layer {
    pub image: RgbaImage,
    pub opacity: f32,
    pub z_index: i32,
}

/// Composite layers onto a canvas of the given size.
/// Layers are sorted by z_index (lowest drawn first / behind).
/// Each layer's alpha is multiplied by its opacity.
pub fn composite(layers: &mut Vec<Layer>, width: u32, height: u32) -> RgbaImage {
    layers.sort_by_key(|l| l.z_index);

    let mut canvas: RgbaImage = ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 255]));

    for layer in layers.iter() {
        blend_layer(&mut canvas, &layer.image, layer.opacity, width, height);
    }

    canvas
}

/// Blend a source layer onto the destination using Porter-Duff "over" with opacity.
/// Source is scaled/cropped to fit the destination if sizes differ.
fn blend_layer(dst: &mut RgbaImage, src: &RgbaImage, opacity: f32, width: u32, height: u32) {
    let (sw, sh) = src.dimensions();

    if opacity <= 0.0 {
        return;
    }

    // If source matches destination size, fast path (no scaling)
    if sw == width && sh == height {
        blend_direct(dst, src, opacity);
    } else {
        // Scale source to fit destination using nearest-neighbor (fast)
        let scaled = image::imageops::resize(src, width, height, image::imageops::FilterType::Nearest);
        blend_direct(dst, &scaled, opacity);
    }
}

/// Direct pixel-by-pixel alpha blend (src over dst) with opacity multiplier.
fn blend_direct(dst: &mut RgbaImage, src: &RgbaImage, opacity: f32) {
    let dst_buf: &mut [u8] = dst.as_mut();
    let src_buf: &[u8] = src.as_ref();
    let len = dst_buf.len().min(src_buf.len());

    // Process 4 bytes at a time (RGBA)
    let mut i = 0;
    while i + 3 < len {
        let sr = src_buf[i] as f32;
        let sg = src_buf[i + 1] as f32;
        let sb = src_buf[i + 2] as f32;
        let sa = (src_buf[i + 3] as f32 / 255.0) * opacity;

        if sa <= 0.0 {
            i += 4;
            continue;
        }

        let dr = dst_buf[i] as f32;
        let dg = dst_buf[i + 1] as f32;
        let db = dst_buf[i + 2] as f32;
        let da = dst_buf[i + 3] as f32 / 255.0;

        let out_a = sa + da * (1.0 - sa);

        if out_a > 0.0 {
            dst_buf[i] = ((sr * sa + dr * da * (1.0 - sa)) / out_a) as u8;
            dst_buf[i + 1] = ((sg * sa + dg * da * (1.0 - sa)) / out_a) as u8;
            dst_buf[i + 2] = ((sb * sa + db * da * (1.0 - sa)) / out_a) as u8;
            dst_buf[i + 3] = (out_a * 255.0) as u8;
        }

        i += 4;
    }
}
