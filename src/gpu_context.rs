use std::sync::Arc;

/// Shared GPU state: device, queue, and compiled compute pipelines.
/// Created once at startup, wrapped in Arc, passed to each channel.
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub blend_pipeline: wgpu::ComputePipeline,
    pub blend_layout: wgpu::BindGroupLayout,
    pub clear_pipeline: wgpu::ComputePipeline,
    pub clear_layout: wgpu::BindGroupLayout,
    pub filter_layout: wgpu::BindGroupLayout,
    pub filter_pipeline_layout: wgpu::PipelineLayout,
}

impl GpuContext {
    /// Try to initialize GPU. Returns None if Metal/GPU unavailable.
    pub fn try_new() -> Option<Arc<Self>> {
        pollster::block_on(Self::init_async())
    }

    async fn init_async() -> Option<Arc<Self>> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..Default::default()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await;

        let adapter = match adapter {
            Ok(a) => {
                let info = a.get_info();
                tracing::info!("GPU adapter: {} ({:?})", info.name, info.backend);
                a
            }
            Err(e) => {
                tracing::warn!("No GPU adapter found: {}, using CPU compositor", e);
                return None;
            }
        };

        let (device, queue): (wgpu::Device, wgpu::Queue) = match adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("ndimixer"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
        {
            Ok(dq) => dq,
            Err(e) => {
                tracing::warn!("GPU device creation failed: {}, using CPU compositor", e);
                return None;
            }
        };

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blend.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/blend.wgsl").into()),
        });

        // Clear pipeline layout: storage texture (write) + uniform params
        let clear_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("clear_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let clear_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("clear_pl"),
            bind_group_layouts: &[&clear_layout],
            immediate_size: 0,
        });

        let clear_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("clear"),
            layout: Some(&clear_pipeline_layout),
            module: &shader,
            entry_point: Some("clear"),
            compilation_options: Default::default(),
            cache: None,
        });

        // Blend pipeline layout: src texture + layer texture + dst storage + uniform params
        let blend_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blend_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let blend_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blend_pl"),
            bind_group_layouts: &[&blend_layout],
            immediate_size: 0,
        });

        let blend_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("blend"),
            layout: Some(&blend_pipeline_layout),
            module: &shader,
            entry_point: Some("blend"),
            compilation_options: Default::default(),
            cache: None,
        });

        // Filter pipeline layout: input texture (read) + output storage (write) + uniform buffer
        let filter_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("filter_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let filter_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("filter_pl"),
            bind_group_layouts: &[&filter_layout],
            immediate_size: 0,
        });

        tracing::info!("GPU compute compositor initialized");

        Some(Arc::new(Self {
            device,
            queue,
            blend_pipeline,
            blend_layout,
            clear_pipeline,
            clear_layout,
            filter_layout,
            filter_pipeline_layout,
        }))
    }

    /// Compile a filter compute shader from WGSL source code.
    pub fn compile_filter_pipeline(
        &self,
        label: &str,
        wgsl_source: &str,
    ) -> Result<wgpu::ComputePipeline, String> {
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(label),
            source: wgpu::ShaderSource::Wgsl(wgsl_source.into()),
        });

        let pipeline = self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(label),
            layout: Some(&self.filter_pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Ok(pipeline)
    }
}
