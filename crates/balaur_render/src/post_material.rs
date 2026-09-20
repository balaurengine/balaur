//! A `material` asset drawn over the whole frame: the user half of
//! `camera.post`.
//!
//! The engine's own passes live in the fork's HDR pipeline and run where the
//! pipeline puts them. A pass named here is a shader the project wrote, and
//! `camera.post` decides both the order of these and which side of the tonemap
//! each falls on: before it the input is the HDR film in linear light, after it
//! the tonemapped picture. Each stage renders to a different texture format, so
//! a material used on both sides is compiled once per format.
//!
//! Nothing here reads the scene. A pass gets one texture, its own `params`, and
//! the screen; that is what makes the order the only thing that matters.

use kiss3d::context::Context;
use kiss3d::post_processing::{PostProcessingContext, PostProcessingEffect};
use kiss3d::resource::RenderTarget;
use kiss3d::wgpu;

use crate::material::Compiled;

/// Matches `PostFrame` in `shaders/post.wesl`.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct PostFrame {
    size_clock: [f32; 4],
}

/// One compiled `camera.post` material, ready to draw over a frame.
pub(crate) struct PostMaterial {
    pipeline: wgpu::RenderPipeline,
    frame_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    frame_uniform: wgpu::Buffer,
    params: Option<wgpu::BindGroup>,
    /// The render clock a pass reads. Started when the pass was built, which
    /// is close enough for anything that moves.
    started: balaur_core::time::Instant,
    /// The frame's size in pixels, as the chain last reported it.
    size: (f32, f32),
}

/// Which group the material's own `Params` land in. Group 0 is the frame.
const PARAMS_GROUP: u32 = 1;

impl PostMaterial {
    /// Build the pipeline for one linked material, writing `format`.
    pub(crate) fn new(compiled: &Compiled, format: wgpu::TextureFormat) -> Self {
        let ctxt = Context::get();
        let frame_layout = ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post_frame_layout"),
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
                crate::bind_layout::uniform_entry(2),
            ],
        });
        // Clamped, because a pass that reads its neighbours reads past the edge
        // at the edge, and a wrapped read there is the far side of the screen.
        let sampler = ctxt.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post_frame_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let params = crate::shader_material::post_params_group(&compiled.params);
        let mut groups = vec![Some(&frame_layout)];
        if let Some((layout, _)) = params.as_ref() {
            debug_assert_eq!(groups.len() as u32, PARAMS_GROUP);
            groups.push(Some(layout));
        }
        let pipeline_layout = ctxt.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post_pipeline_layout"),
            bind_group_layouts: &groups,
            immediate_size: 0,
        });
        let shader = ctxt.create_shader_module(Some("post_material_shader"), &compiled.wgsl);
        let pipeline = ctxt.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("post_material_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                // The triangle comes from the vertex index; there is nothing to
                // feed it.
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            frame_layout,
            sampler,
            frame_uniform: ctxt.create_buffer(&wgpu::BufferDescriptor {
                label: Some("post_frame_uniform"),
                size: std::mem::size_of::<PostFrame>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            params: params.map(|(_, group)| group),
            // Render time, outside the simulation: a pass that moves must
            // never be something a replay can disagree about.
            #[allow(clippy::disallowed_methods)]
            started: balaur_core::time::Instant::now(),
            size: (1.0, 1.0),
        }
    }
}

impl PostProcessingEffect for PostMaterial {
    fn update(&mut self, _dt: f32, w: f32, h: f32, _znear: f32, _zfar: f32) {
        // The only place the chain says how big the frame is. `dt` is a fixed
        // step here whatever the frame took, so the clock comes from a real
        // one below rather than from accumulating this.
        self.size = (w.max(1.0), h.max(1.0));
    }

    fn draw(&mut self, target: &RenderTarget, context: &mut PostProcessingContext<'_>) {
        let Some(input) = target.color_view() else {
            // The screen is not a texture a pass can read; the chain that put
            // us here always hands an offscreen target.
            return;
        };
        let ctxt = Context::get();
        ctxt.write_buffer(
            &self.frame_uniform,
            0,
            bytemuck::bytes_of(&PostFrame {
                size_clock: [
                    self.size.0,
                    self.size.1,
                    self.started.elapsed().as_secs_f32(),
                    0.0,
                ],
            }),
        );
        let frame = ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post_frame_bind_group"),
            layout: &self.frame_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(input),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.frame_uniform.as_entire_binding(),
                },
            ],
        });
        let mut pass = context
            .encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post_material_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: context.output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // The pass writes every pixel, so what was there is not
                        // worth the bandwidth of loading.
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &frame, &[]);
        if let Some(params) = self.params.as_ref() {
            pass.set_bind_group(PARAMS_GROUP, params, &[]);
        }
        pass.draw(0..3, 0..1);
    }
}

/// One pass on a camera's chain, as built.
///
/// A concrete enum rather than a box: the fork's chain takes
/// `&mut [&mut dyn PostProcessingEffect]`, and a boxed trait object cannot
/// shorten its own lifetime behind a mutable reference to land there.
pub(crate) enum Pass {
    /// A shader a project wrote, or one of the engine's finishing passes.
    Material(PostMaterial),
    Fxaa(kiss3d::post_processing::Fxaa),
    Sharpen(kiss3d::post_processing::Cas),
}

impl PostProcessingEffect for Pass {
    fn update(&mut self, dt: f32, w: f32, h: f32, znear: f32, zfar: f32) {
        match self {
            Self::Material(pass) => pass.update(dt, w, h, znear, zfar),
            Self::Fxaa(pass) => pass.update(dt, w, h, znear, zfar),
            Self::Sharpen(pass) => pass.update(dt, w, h, znear, zfar),
        }
    }

    fn draw(&mut self, target: &RenderTarget, context: &mut PostProcessingContext<'_>) {
        match self {
            Self::Material(pass) => pass.draw(target, context),
            Self::Fxaa(pass) => pass.draw(target, context),
            Self::Sharpen(pass) => pass.draw(target, context),
        }
    }
}

/// What a chain was built from. A change in any of it rebuilds both sides,
/// which is cheaper than working out which pass each move touched.
type Built = (Vec<String>, Vec<String>, u64, [u32; 5]);

/// The `camera.post` materials as built, each side of the tonemap.
///
/// Rebuilt when the camera's list changes or an asset is saved, and not
/// otherwise: a pipeline per pass per frame would cost more than every pass
/// it draws.
#[derive(Default)]
pub(crate) struct PostChain {
    pub(crate) film: Vec<Pass>,
    pub(crate) screen: Vec<Pass>,
    /// What these were built from: the two lists, the asset generation, and
    /// the finishing knobs.
    built: Option<Built>,
}

impl PostChain {
    /// Build the two chains if what they were built from has moved.
    ///
    /// A material that fails to compile is reported once and left out, so a
    /// typo in a post shader costs that pass and not the frame.
    pub(crate) fn sync(
        &mut self,
        app: &balaur_core::App,
        screen_format: wgpu::TextureFormat,
        generation: u64,
    ) {
        let Some(config) = app.engine.try_resource::<crate::PostConfig>() else {
            return;
        };
        let (film, screen, finish) = {
            let config = config.borrow();
            (config.film.clone(), config.screen.clone(), config.finish)
        };
        let wanted = (film, screen, generation, finish.bits());
        if self.built.as_ref() == Some(&wanted) {
            return;
        }
        let (film, screen, _, _) = &wanted;
        self.film = build_all(app, film, kiss3d::post_processing::HDR_FORMAT, &finish);
        self.screen = build_all(app, screen, screen_format, &finish);
        self.built = Some(wanted);
    }
}

fn build_all(
    app: &balaur_core::App,
    ids: &[String],
    format: wgpu::TextureFormat,
    finish: &crate::Finish,
) -> Vec<Pass> {
    ids.iter()
        .filter_map(|id| match build(app, id, format, finish) {
            Ok(material) => Some(material),
            Err(why) => {
                tracing::error!("camera post pass '{id}': {why:#}");
                None
            }
        })
        .collect()
}

fn build(
    app: &balaur_core::App,
    reference: &str,
    format: wgpu::TextureFormat,
    finish: &crate::Finish,
) -> anyhow::Result<Pass> {
    use crate::vocabulary::words;
    // Two the fork already owns, drawn where the list puts them rather than
    // at a fixed place in the pipeline.
    match reference {
        words::FXAA => return Ok(Pass::Fxaa(kiss3d::post_processing::Fxaa::new())),
        // Half sharpness: enough to put back what a smoothing pass took out,
        // short of the ringing the full amount draws around an edge.
        words::SHARPEN => return Ok(Pass::Sharpen(kiss3d::post_processing::Cas::new(0.5))),
        _ => {}
    }
    // The engine's own finishing passes are materials too; what a project
    // does not supply for them is their name, their shader and their values.
    if words::FINISHES.contains(&reference) {
        let material = crate::material::Material3d {
            features: vec![(reference.to_string(), true)],
            params: finish.params(),
            ..crate::material::Material3d::default()
        };
        let modules = crate::shaders::plugin_modules(&app.engine);
        let compiled = crate::material::compile_with(&material, crate::shaders::FINISH, &modules)?;
        return Ok(Pass::Material(PostMaterial::new(&compiled, format)));
    }
    let asset =
        balaur_core::assets::load_typed::<crate::material::Material3d>(&app.engine, reference)?;
    let source = crate::material::shader_text(&app.engine, reference, &asset.shader)?;
    let modules = crate::shaders::plugin_modules(&app.engine);
    let compiled = crate::material::compile_with(&asset, &source, &modules)?;
    Ok(Pass::Material(PostMaterial::new(&compiled, format)))
}
