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

use crate::material::Compiled;

/// One compiled `camera.post` material, ready to draw over a frame.
pub(crate) struct PostMaterial {
    pipeline: wgpu::RenderPipeline,
    frame_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params: Option<wgpu::BindGroup>,
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
            params: params.map(|(_, group)| group),
        }
    }
}

impl PostProcessingEffect for PostMaterial {
    fn update(&mut self, _dt: f32, _w: f32, _h: f32, _znear: f32, _zfar: f32) {}

    fn draw(&mut self, target: &RenderTarget, context: &mut PostProcessingContext<'_>) {
        let Some(input) = target.color_view() else {
            // The screen is not a texture a pass can read; the chain that put
            // us here always hands an offscreen target.
            return;
        };
        let ctxt = Context::get();
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

/// The `camera.post` materials as built, each side of the tonemap.
///
/// Rebuilt when the camera's list changes or an asset is saved, and not
/// otherwise: a pipeline per pass per frame would cost more than every pass
/// it draws.
#[derive(Default)]
pub(crate) struct PostChain {
    pub(crate) film: Vec<PostMaterial>,
    pub(crate) screen: Vec<PostMaterial>,
    /// The lists and the asset generation these were built from.
    built: Option<(Vec<String>, Vec<String>, u64)>,
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
        let (film, screen) = {
            let config = config.borrow();
            (config.film.clone(), config.screen.clone())
        };
        let wanted = (film, screen, generation);
        if self.built.as_ref() == Some(&wanted) {
            return;
        }
        let (film, screen, _) = &wanted;
        self.film = build_all(app, film, kiss3d::post_processing::HDR_FORMAT);
        self.screen = build_all(app, screen, screen_format);
        self.built = Some(wanted);
    }
}

fn build_all(
    app: &balaur_core::App,
    ids: &[String],
    format: wgpu::TextureFormat,
) -> Vec<PostMaterial> {
    ids.iter()
        .filter_map(|id| match build(app, id, format) {
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
) -> anyhow::Result<PostMaterial> {
    let asset =
        balaur_core::assets::load_typed::<crate::material::Material>(&app.engine, reference)?;
    let source = crate::material::shader_text(&app.engine, reference, &asset.shader)?;
    let modules = crate::shaders::plugin_modules(&app.engine);
    let compiled = crate::material::compile_with(&asset, &source, &modules)?;
    Ok(PostMaterial::new(&compiled, format))
}
