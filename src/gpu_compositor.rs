use image::RgbaImage;
use std::sync::Arc;
use wgpu::util::DeviceExt;

use crate::compositor::Layer;
use crate::gpu_context::GpuContext;

/// Uniform buffer matching the WGSL Params struct (16-byte aligned).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct BlendParams {
    opacity: f32,
    width: u32,
    height: u32,
    _pad: u32,
}

struct CachedTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
}

/// Per-channel GPU compositor. Owns ping-pong textures, staging buffer,
/// and a layer texture cache. Not shared across channels.
pub struct GpuCompositor {
    ctx: Arc<GpuContext>,
    ping: wgpu::Texture,
    ping_view: wgpu::TextureView,
    pong: wgpu::Texture,
    pong_view: wgpu::TextureView,
    staging: wgpu::Buffer,
    layer_cache: Vec<Option<CachedTexture>>,
    width: u32,
    height: u32,
    padded_row: u32,
}

impl GpuCompositor {
    pub fn new(ctx: Arc<GpuContext>, width: u32, height: u32) -> Self {
        let device = &ctx.device;

        let tex_usage = wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST;

        let tex_desc = wgpu::TextureDescriptor {
            label: Some("ping"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: tex_usage,
            view_formats: &[],
        };

        let ping = device.create_texture(&tex_desc);
        let pong = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pong"),
            ..tex_desc
        });

        let ping_view = ping.create_view(&Default::default());
        let pong_view = pong.create_view(&Default::default());

        // Staging buffer for GPU→CPU readback (padded rows)
        let padded_row = (width * 4 + 255) & !255;
        let staging_size = (padded_row as u64) * (height as u64);

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging"),
            size: staging_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Self {
            ctx,
            ping,
            ping_view,
            pong,
            pong_view,
            staging,
            layer_cache: Vec::new(),
            width,
            height,
            padded_row,
        }
    }

    /// Composite layers onto canvas using GPU compute shaders.
    /// Returns true on success. On failure, caller should fall back to CPU.
    pub fn composite(&mut self, canvas: &mut RgbaImage, layers: &mut [Layer]) -> bool {
        layers.sort_by_key(|l| l.z_index);

        let dispatch_x = (self.width + 15) / 16;
        let dispatch_y = (self.height + 15) / 16;

        // Upload all layer textures first (needs &mut self)
        for (i, layer) in layers.iter().enumerate() {
            if layer.opacity > 0.0 {
                self.upload_layer(i, &layer.image);
            }
        }

        // Now borrow ctx immutably for the rest
        let device = &self.ctx.device;

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        // Step 1: Clear ping to opaque black
        let clear_params = BlendParams {
            opacity: 0.0,
            width: self.width,
            height: self.height,
            _pad: 0,
        };
        let clear_params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&clear_params),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let clear_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.ctx.clear_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.ping_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: clear_params_buf.as_entire_binding(),
                },
            ],
        });

        {
            let mut pass =
                encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
            pass.set_pipeline(&self.ctx.clear_pipeline);
            pass.set_bind_group(0, &clear_bg, &[]);
            pass.dispatch_workgroups(dispatch_x, dispatch_y, 1);
        }

        // Step 2: Blend each layer (ping-pong)
        let mut ping_is_src = true;

        for (i, layer) in layers.iter().enumerate() {
            if layer.opacity <= 0.0 {
                continue;
            }

            let layer_view = &self.layer_cache[i].as_ref().unwrap().view;

            let params = BlendParams {
                opacity: layer.opacity,
                width: self.width,
                height: self.height,
                _pad: 0,
            };
            let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });

            let (src_view, dst_view) = if ping_is_src {
                (&self.ping_view, &self.pong_view)
            } else {
                (&self.pong_view, &self.ping_view)
            };

            let blend_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.ctx.blend_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(src_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(layer_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(dst_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: params_buf.as_entire_binding(),
                    },
                ],
            });

            {
                let mut pass = encoder
                    .begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
                pass.set_pipeline(&self.ctx.blend_pipeline);
                pass.set_bind_group(0, &blend_bg, &[]);
                pass.dispatch_workgroups(dispatch_x, dispatch_y, 1);
            }

            ping_is_src = !ping_is_src;
        }

        // Step 3: Copy result to staging buffer
        let result_tex = if ping_is_src {
            &self.ping
        } else {
            &self.pong
        };

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: result_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );

        self.ctx.queue.submit(std::iter::once(encoder.finish()));

        // Step 4: Readback — map staging buffer, copy into canvas
        let slice = self.staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = self.ctx.device.poll(wgpu::PollType::wait_indefinitely());

        match rx.recv() {
            Ok(Ok(())) => {
                let data = slice.get_mapped_range();
                let canvas_buf: &mut [u8] = canvas.as_mut();
                let row_bytes = (self.width * 4) as usize;

                if self.padded_row as usize == row_bytes {
                    canvas_buf.copy_from_slice(&data[..canvas_buf.len()]);
                } else {
                    for y in 0..self.height as usize {
                        let src_off = y * self.padded_row as usize;
                        let dst_off = y * row_bytes;
                        canvas_buf[dst_off..dst_off + row_bytes]
                            .copy_from_slice(&data[src_off..src_off + row_bytes]);
                    }
                }

                drop(data);
                self.staging.unmap();
                true
            }
            _ => {
                tracing::warn!("GPU readback failed, falling back to CPU");
                false
            }
        }
    }

    /// Upload layer image to a cached GPU texture, resizing on CPU if needed.
    fn upload_layer(&mut self, index: usize, image: &RgbaImage) {
        let (img_w, img_h) = image.dimensions();

        // Ensure cache has enough slots
        while self.layer_cache.len() <= index {
            self.layer_cache.push(None);
        }

        // Recreate texture if dimensions don't match canvas
        let needs_recreate = match &self.layer_cache[index] {
            Some(c) => c.width != self.width || c.height != self.height,
            None => true,
        };

        if needs_recreate {
            let texture = self.ctx.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("layer"),
                size: wgpu::Extent3d {
                    width: self.width,
                    height: self.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.layer_cache[index] = Some(CachedTexture {
                texture,
                view,
                width: self.width,
                height: self.height,
            });
        }

        // Resize on CPU if layer doesn't match canvas (same as CPU compositor)
        let upload_data: std::borrow::Cow<[u8]> = if img_w == self.width && img_h == self.height {
            std::borrow::Cow::Borrowed(image.as_raw())
        } else {
            let resized = image::imageops::resize(
                image,
                self.width,
                self.height,
                image::imageops::FilterType::Nearest,
            );
            std::borrow::Cow::Owned(resized.into_raw())
        };

        let cached = self.layer_cache[index].as_ref().unwrap();
        self.ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &cached.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &upload_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.width * 4),
                rows_per_image: Some(self.height),
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
    }
}
