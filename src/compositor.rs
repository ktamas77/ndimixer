use image::RgbaImage;

pub struct Layer {
    pub image: RgbaImage,
    pub opacity: f32,
    pub z_index: i32,
}

/// Composite layers onto a caller-owned canvas (reused across frames).
/// Canvas is cleared to opaque black, then layers are blended by z_index order.
pub fn composite(canvas: &mut RgbaImage, layers: &mut [Layer]) {
    let (width, height) = canvas.dimensions();

    // Clear canvas to opaque black
    let buf: &mut [u8] = canvas.as_mut();
    for pixel in buf.chunks_exact_mut(4) {
        pixel[0] = 0;
        pixel[1] = 0;
        pixel[2] = 0;
        pixel[3] = 255;
    }

    layers.sort_by_key(|l| l.z_index);

    // Fast path: single opaque layer at matching size — just copy
    if layers.len() == 1 && layers[0].opacity >= 1.0 {
        let (sw, sh) = layers[0].image.dimensions();
        if sw == width && sh == height {
            buf.copy_from_slice(layers[0].image.as_ref());
            return;
        }
    }

    for layer in layers.iter() {
        blend_layer(canvas, &layer.image, layer.opacity, width, height);
    }
}

/// Blend a source layer onto the destination using Porter-Duff "over" with opacity.
fn blend_layer(dst: &mut RgbaImage, src: &RgbaImage, opacity: f32, width: u32, height: u32) {
    let (sw, sh) = src.dimensions();

    if opacity <= 0.0 {
        return;
    }

    if sw == width && sh == height {
        blend_direct(dst, src, opacity);
    } else {
        let scaled =
            image::imageops::resize(src, width, height, image::imageops::FilterType::Nearest);
        blend_direct(dst, &scaled, opacity);
    }
}

/// Integer-based pixel-by-pixel alpha blend (src over dst) with opacity multiplier.
/// Uses u16 arithmetic instead of f32 to avoid float overhead.
fn blend_direct(dst: &mut RgbaImage, src: &RgbaImage, opacity: f32) {
    let dst_buf: &mut [u8] = dst.as_mut();
    let src_buf: &[u8] = src.as_ref();
    let len = dst_buf.len().min(src_buf.len());

    // Pre-convert opacity to 0..256 fixed-point
    let opa = (opacity * 256.0) as u16;

    let mut i = 0;
    while i + 3 < len {
        // Source alpha * opacity in 0..255 range
        let raw_sa = src_buf[i + 3] as u16;
        let sa = (raw_sa * opa) >> 8; // 0..255

        if sa == 0 {
            i += 4;
            continue;
        }

        // Fully opaque source — just copy (common case for video)
        if sa >= 255 {
            dst_buf[i] = src_buf[i];
            dst_buf[i + 1] = src_buf[i + 1];
            dst_buf[i + 2] = src_buf[i + 2];
            dst_buf[i + 3] = 255;
            i += 4;
            continue;
        }

        let inv_sa = 255 - sa; // 0..255
        let da = dst_buf[i + 3] as u16;

        // out_a = sa + da * (1 - sa/255), scaled to 0..255
        let out_a = sa + ((da * inv_sa) >> 8);

        if out_a > 0 {
            // Blend each channel: (src * sa + dst * da * inv_sa / 255) / out_a
            let sr = src_buf[i] as u16;
            let sg = src_buf[i + 1] as u16;
            let sb = src_buf[i + 2] as u16;
            let dr = dst_buf[i] as u16;
            let dg = dst_buf[i + 1] as u16;
            let db = dst_buf[i + 2] as u16;

            let da_inv = (da * inv_sa) >> 8;

            dst_buf[i] = ((sr * sa + dr * da_inv) / out_a) as u8;
            dst_buf[i + 1] = ((sg * sa + dg * da_inv) / out_a) as u8;
            dst_buf[i + 2] = ((sb * sa + db * da_inv) / out_a) as u8;
            dst_buf[i + 3] = out_a as u8;
        }

        i += 4;
    }
}
