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
use kiss3d::post_processing::{self as post, PostProcessingContext, PostProcessingEffect};
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
    Fxaa(post::Fxaa),
    Sharpen(post::Cas),
    Crt(post::Crt),
    Grayscale(post::Grayscales),
    Waves(post::Waves),
    Loupe(Box<post::Loupe>),
    Stereo(post::OculusStereo),
    Edges(post::SobelEdgeHighlight),
    Gi(Box<post::Gi2d>),
}

impl Pass {
    fn effect(&mut self) -> &mut dyn PostProcessingEffect {
        match self {
            Self::Material(pass) => pass,
            Self::Fxaa(pass) => pass,
            Self::Sharpen(pass) => pass,
            Self::Crt(pass) => pass,
            Self::Grayscale(pass) => pass,
            Self::Waves(pass) => pass,
            Self::Loupe(pass) => pass.as_mut(),
            Self::Stereo(pass) => pass,
            Self::Edges(pass) => pass,
            Self::Gi(pass) => pass.as_mut(),
        }
    }
}

impl Pass {
    fn effect_ref(&self) -> &dyn PostProcessingEffect {
        match self {
            Self::Material(pass) => pass,
            Self::Fxaa(pass) => pass,
            Self::Sharpen(pass) => pass,
            Self::Crt(pass) => pass,
            Self::Grayscale(pass) => pass,
            Self::Waves(pass) => pass,
            Self::Loupe(pass) => pass.as_ref(),
            Self::Stereo(pass) => pass,
            Self::Edges(pass) => pass,
            Self::Gi(pass) => pass.as_ref(),
        }
    }
}

// Every method forwards, the defaulted ones too: a wrapper that kept the
// default would answer for the effect inside it and hide what it reads.
impl PostProcessingEffect for Pass {
    fn update(&mut self, dt: f32, w: f32, h: f32, znear: f32, zfar: f32) {
        self.effect().update(dt, w, h, znear, zfar);
    }

    fn draw(&mut self, target: &RenderTarget, context: &mut PostProcessingContext<'_>) {
        self.effect().draw(target, context);
    }

    fn reads_depth(&self) -> bool {
        self.effect_ref().reads_depth()
    }

    fn set_camera_2d(&mut self, camera: &dyn kiss3d::camera::Camera2d) {
        self.effect().set_camera_2d(camera);
    }
}

/// What a chain was built from. A change in any of it rebuilds both sides,
/// which is cheaper than working out which pass each move touched.
type Built = (Vec<String>, Vec<String>, u64, [u32; 5], Vec<u32>);

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
    /// the knobs the passes are built with.
    built: Option<Built>,
}

/// What every pass on a chain is built with.
struct Knobs {
    finish: crate::Finish,
    effects: crate::Effects,
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
        let (film, screen, knobs) = {
            let config = config.borrow();
            let knobs = Knobs {
                finish: config.finish,
                effects: config.effects,
            };
            (config.film.clone(), config.screen.clone(), knobs)
        };
        let wanted = (
            film,
            screen,
            generation,
            knobs.finish.bits(),
            knobs.effects.bits(),
        );
        if self.built.as_ref() == Some(&wanted) {
            return;
        }
        let (film, screen, ..) = &wanted;
        self.film = build_all(app, film, post::HDR_FORMAT, &knobs);
        self.screen = build_all(app, screen, screen_format, &knobs);
        self.built = Some(wanted);
    }
}

fn build_all(
    app: &balaur_core::App,
    ids: &[String],
    format: wgpu::TextureFormat,
    knobs: &Knobs,
) -> Vec<Pass> {
    ids.iter()
        .filter_map(|id| match build(app, id, format, knobs) {
            Ok(pass) => Some(pass),
            Err(why) => {
                tracing::error!("camera post pass '{id}': {why:#}");
                None
            }
        })
        .collect()
}

/// One of the passes kiss3d draws itself, built with its knobs; `None` for a
/// name that is not one.
fn effect(name: &str, effects: &crate::Effects) -> Option<Pass> {
    use crate::vocabulary::words;
    let pass = match name {
        words::FXAA => {
            let mut fxaa = post::Fxaa::new();
            fxaa.set_thresholds(effects.fxaa_edge_threshold, effects.fxaa_edge_threshold_min);
            Pass::Fxaa(fxaa)
        }
        words::SHARPEN => Pass::Sharpen(post::Cas::new(effects.sharpen_amount)),
        words::CRT => {
            let mut crt = post::Crt::new();
            crt.set_curvature(effects.crt_curvature);
            crt.set_aberration(effects.crt_aberration);
            crt.set_scanlines(effects.crt_scanline_intensity, effects.crt_scanline_count);
            crt.set_vignette(effects.crt_vignette);
            Pass::Crt(crt)
        }
        words::GRAYSCALE => Pass::Grayscale(post::Grayscales::new()),
        words::WAVES => Pass::Waves(post::Waves::new()),
        words::LOUPE => Pass::Loupe(Box::new(loupe(effects))),
        words::STEREO => Pass::Stereo(post::OculusStereo::new()),
        words::EDGES => Pass::Edges(post::SobelEdgeHighlight::new(effects.edges_threshold)),
        words::GI => Pass::Gi(Box::new(gi(&effects.gi))),
        _ => return None,
    };
    Some(pass)
}

fn gi(knobs: &crate::Gi) -> post::Gi2d {
    let mut gi = post::Gi2d::new();
    gi.set_rays(knobs.rays);
    gi.set_max_distance(knobs.max_distance);
    gi.set_max_steps(knobs.max_steps);
    gi.set_resolution_scale(knobs.downscale);
    gi.set_temporal_blend(knobs.temporal_blend);
    gi.set_radiance_cascades(knobs.cascades);
    gi.set_cascade_count(knobs.cascade_count);
    gi.set_cascade_base_directions(knobs.cascade_directions);
    gi.set_sdf_occluders(knobs.screen_occluders);
    gi.set_cascade_probe_spacing(knobs.probe_spacing);
    gi
}

/// Hand every `gi` pass this frame's lights, occluder outlines and ambient.
pub(crate) fn feed_gi(chain: &mut PostChain, app: &balaur_core::App) {
    let mut passes: Vec<&mut post::Gi2d> = chain
        .film
        .iter_mut()
        .chain(chain.screen.iter_mut())
        .filter_map(|pass| match pass {
            Pass::Gi(gi) => Some(gi.as_mut()),
            _ => None,
        })
        .collect();
    if passes.is_empty() {
        return;
    }
    let root = app.engine.root();
    let emitters: Vec<post::GiEmitter2d> = {
        let world = app.engine.world();
        balaur_core::scene::collect_subtree(&world, root)
            .into_iter()
            .filter_map(|entity| {
                let light = world.get::<&crate::light::Light2d>(entity).ok()?;
                let global = world.get::<&balaur_core::GlobalTransform>(entity).ok()?;
                let [r, g, b, _] = light.color;
                (light.kind != crate::light::LightKind2d::Directional).then(|| {
                    post::GiEmitter2d::new(
                        glamx::Vec2::new(global.position.x, global.position.y),
                        light.source_radius,
                        kiss3d::color::Color::new(r, g, b, 1.0),
                        light.intensity,
                    )
                })
            })
            .collect()
    };
    let segments: Vec<post::GiSegmentOccluder2d> = crate::light::occluder_edges(&app.engine, root)
        .into_iter()
        .map(|[a, b]| post::GiSegmentOccluder2d::new(a, b, 0.0))
        .collect();
    let [r, g, b] = app
        .engine
        .try_resource::<crate::CameraConfig2d>()
        .map_or([0.0; 3], |config| config.borrow().ambient);
    for gi in &mut passes {
        gi.set_emitters(&emitters);
        gi.set_segment_occluders(&segments);
        gi.set_ambient(kiss3d::color::Color::new(r, g, b, 1.0));
    }
}

/// Whether the current camera lights its frame with `gi`, which takes the
/// place of the `light2d` light map.
pub(crate) fn lit_by_gi(app: &balaur_core::App) -> bool {
    app.engine
        .try_resource::<crate::PostConfig>()
        .is_some_and(|config| {
            let config = config.borrow();
            let gi = crate::vocabulary::words::GI;
            config
                .film
                .iter()
                .chain(&config.screen)
                .any(|pass| pass == gi)
        })
}

fn loupe(effects: &crate::Effects) -> post::Loupe {
    let mut loupe = post::Loupe::new();
    loupe.set_zoom(effects.loupe_zoom);
    loupe.set_focus(glamx::Vec2::from(effects.loupe_focus));
    loupe.set_corner(match effects.loupe_corner {
        crate::LoupeCorner::TopLeft => post::LoupeCorner::TopLeft,
        crate::LoupeCorner::TopRight => post::LoupeCorner::TopRight,
        crate::LoupeCorner::BottomLeft => post::LoupeCorner::BottomLeft,
        crate::LoupeCorner::BottomRight => post::LoupeCorner::BottomRight,
    });
    loupe.set_size(effects.loupe_size);
    loupe.set_border_color(effects.loupe_border_color);
    loupe
}

fn build(
    app: &balaur_core::App,
    reference: &str,
    format: wgpu::TextureFormat,
    knobs: &Knobs,
) -> anyhow::Result<Pass> {
    use crate::vocabulary::words;
    // The fork's own passes, drawn where the list puts them rather than at a
    // fixed place in the pipeline.
    if let Some(pass) = effect(reference, &knobs.effects) {
        return Ok(pass);
    }
    // The engine's own finishing passes are materials too; what a project
    // does not supply for them is their name, their shader and their values.
    if words::FINISHES.contains(&reference) {
        let material = crate::material::Material3d {
            features: vec![(reference.to_string(), true)],
            params: knobs.finish.params(),
            ..crate::material::Material3d::default()
        };
        let modules = crate::shaders::plugin_modules(&app.engine);
        let compiled = crate::material::compile_with(&material, crate::shaders::FINISH, &modules)?;
        return Ok(Pass::Material(PostMaterial::new(&compiled, format)));
    }
    let asset =
        balaur_core::assets::load_typed::<crate::material::Material3d>(&app.engine, reference)?;
    if asset.shader.is_empty() {
        anyhow::bail!("'{reference}' names no shader, and a post pass draws one");
    }
    let source = crate::material::shader_text(&app.engine, reference, &asset.shader)?;
    let modules = crate::shaders::plugin_modules(&app.engine);
    let compiled = crate::material::compile_with(&asset, &source, &modules)?;
    Ok(Pass::Material(PostMaterial::new(&compiled, format)))
}

/// Apply the screen-space effects the current `camera` asked for.
///
/// Only on the edge: kiss3d rebuilds its post chain when one of these
/// switches, so re-asserting them every frame would rebuild it every frame.
pub(crate) fn apply_post(app: &balaur_core::App, window: &mut kiss3d::window::Window) {
    let Some(post) = app.engine.try_resource::<crate::PostConfig>() else {
        return;
    };
    let mut post = post.borrow_mut();
    if !post.changed {
        return;
    }
    post.changed = false;
    window.set_bloom_enabled(post.bloom);
    window.set_bloom(post.bloom_threshold, post.bloom_intensity);
    window.hdr_settings_mut().bloom_knee = post.bloom_knee;
    window.hdr_settings_mut().bloom_mips = post.bloom_mips;
    window.set_ssao_enabled(post.ssao);
    // Each pass's settings only while it is on: asking for them builds the
    // pass's state, and a scene that never uses one should not pay for it.
    if post.ssao {
        let ssao = window.ssao_settings_mut();
        ssao.radius = post.occlusion.radius;
        ssao.bias = post.occlusion.bias;
        ssao.intensity = post.occlusion.intensity;
        ssao.power = post.occlusion.power;
    }
    window.set_ssr_enabled(post.ssr);
    if post.ssr {
        let (want, ssr) = (post.reflections, window.ssr_settings_mut());
        ssr.max_steps = want.max_steps;
        ssr.thickness = want.thickness;
        ssr.max_distance = want.max_distance;
        ssr.roughness_cutoff = want.roughness_cutoff;
        ssr.edge_fade = want.edge_fade;
        ssr.intensity = want.intensity;
    }
    window.set_dof_enabled(post.dof);
    if post.dof {
        let (want, dof) = (post.depth_of_field, window.dof_settings_mut());
        dof.mode = match want.mode {
            crate::FocusBlur::Bokeh => kiss3d::renderer::DepthOfFieldMode::Bokeh,
            crate::FocusBlur::Gaussian => kiss3d::renderer::DepthOfFieldMode::Gaussian,
        };
        dof.focal_distance = want.focus_distance;
        dof.aperture_f_stops = want.aperture_f_stops;
        dof.sensor_height = want.sensor_height;
        dof.max_coc_diameter = want.max_blur_pixels;
        dof.max_depth = want.max_depth;
        dof.num_taps = want.taps;
    }
    // Bloom and auto-exposure compile on demand, so a project that never uses
    // them never builds them. Here is where the settings changed, which is a
    // better place to wait for a compiler than the first frame that draws one.
    window.prepare_post();
}
