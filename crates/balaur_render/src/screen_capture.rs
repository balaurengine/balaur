//! The frame as a texture a material may sample: `features = { screen =
//! true }` on a `material` binds the last frame drawn as `screen_texture`.
//!
//! One pass at the end of the frame copies the finished picture into a
//! texture the next frame's materials read. A frame behind, and that on
//! purpose: kiss3d draws every 2D object in one render pass, and a copy in
//! the middle of it would need the pass split. The pass runs only while a
//! material asks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use kiss3d::context::Context;
use kiss3d::post_processing::{PostProcessingContext, PostProcessingEffect};
use kiss3d::resource::RenderTarget;

/// A fullscreen triangle sampling one texture into whatever it is drawn on.
const BLIT: &str = r"
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
struct Out { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> Out {
    var out: Out;
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, 1.0 - (y + 1.0) * 0.5);
    return out;
}
@fragment fn fs_main(in: Out) -> @location(0) vec4<f32> {
    return textureSample(source, source_sampler, in.uv);
}
";

/// What a material binds: the kept frame, or a black pixel before the first
/// frame lands. `generation` counts replacements so a bind group knows to
/// rebuild.
pub(crate) struct ScreenTexture {
    pub(crate) view: wgpu::TextureView,
    pub(crate) sampler: wgpu::Sampler,
    pub(crate) generation: u64,
}

pub(crate) type ScreenShare = Rc<RefCell<ScreenTexture>>;

impl ScreenTexture {
    pub(crate) fn share() -> ScreenShare {
        let ctxt = Context::get();
        let black = ctxt.create_texture(&wgpu::TextureDescriptor {
            label: Some("screen_texture_placeholder"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let sampler = ctxt.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("screen_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Rc::new(RefCell::new(Self {
            view: black.create_view(&wgpu::TextureViewDescriptor::default()),
            sampler,
            generation: 0,
        }))
    }
}

/// The copy pass: blits the finished frame into the kept texture, then on
/// through to the output as the post chain expects.
pub(crate) struct ScreenCapture {
    share: ScreenShare,
    kept: Option<(
        wgpu::Texture,
        wgpu::TextureView,
        u32,
        u32,
        wgpu::TextureFormat,
    )>,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    /// One pipeline per target format: the kept texture's and the output's.
    pipelines: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
}

impl ScreenCapture {
    pub(crate) fn new(share: ScreenShare) -> Self {
        let ctxt = Context::get();
        let layout = ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen_blit_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = ctxt.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("screen_blit_pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = ctxt.create_shader_module(Some("screen_blit"), BLIT);
        Self {
            share,
            kept: None,
            layout,
            pipeline_layout,
            shader,
            pipelines: HashMap::new(),
        }
    }

    fn pipeline(&mut self, format: wgpu::TextureFormat) -> &wgpu::RenderPipeline {
        let (layout, shader) = (&self.pipeline_layout, &self.shader);
        self.pipelines.entry(format).or_insert_with(|| {
            Context::get().create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("screen_blit_pipeline"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    /// The kept texture at the frame's size and format, made anew when
    /// either changes; every material's bind group is then a generation old.
    fn keep(&mut self, width: u32, height: u32, format: wgpu::TextureFormat) -> wgpu::TextureView {
        if let Some((_, view, w, h, f)) = &self.kept {
            if *w == width && *h == height && *f == format {
                return view.clone();
            }
        }
        let texture = Context::get().create_texture(&wgpu::TextureDescriptor {
            label: Some("screen_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        {
            let mut share = self.share.borrow_mut();
            share.view = view.clone();
            share.generation += 1;
        }
        self.kept = Some((texture, view.clone(), width, height, format));
        view
    }

    fn blit(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) {
        let bind_group = Context::get().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("screen_blit_bind_group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        let pipeline = self.pipeline(format).clone();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("screen_blit_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

impl PostProcessingEffect for ScreenCapture {
    fn update(&mut self, _dt: f32, _w: f32, _h: f32, _znear: f32, _zfar: f32) {}

    fn draw(&mut self, target: &RenderTarget, context: &mut PostProcessingContext<'_>) {
        let RenderTarget::Offscreen(frame) = target else {
            return;
        };
        let format = frame.color_texture.format();
        let kept = self.keep(frame.width, frame.height, format);
        self.blit(
            context.encoder,
            &frame.color_view,
            &frame.sampler,
            &kept,
            format,
        );
        let out_format = context.output_view.texture().format();
        self.blit(
            context.encoder,
            &frame.color_view,
            &frame.sampler,
            context.output_view,
            out_format,
        );
    }
}
