//! A project's `material` asset drawn as a kiss3d 3D material.
//!
//! The 3D counterpart of [`crate::shader_material`]: the same four bind groups
//! — frame, object, texture, the material's own values — with the scene's
//! lights and fog folded into the frame one, which is what lets a project's
//! shader light itself without knowing how kiss3d collects them.
//! `shaders/mesh.wesl` is the contract between the two.
//!
//! What this does not carry yet: image-based lighting, reflection probes,
//! SSAO, the transmission background and the clustered light buffers. The
//! fork now offers all five to every registered material
//! (`MaterialManager3d::for_each`); binding them is the next step, and each
//! is another entry in the frame group rather than a new group, because
//! WebGPU guarantees only four.

use std::any::Any;
use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use glamx::{Mat4, Pose3, Vec3};
use kiss3d::camera::Camera3d;
use kiss3d::context::Context;
use kiss3d::light::{FogMode, LightCollection, LightType};
use kiss3d::resource::vertex_index::VERTEX_INDEX_FORMAT;
use kiss3d::resource::{
    multisample_state, GpuData, GpuMesh3d, Material3d, MaterialManager3d, PipelineCache,
    RenderContext, Texture,
};
use kiss3d::scene::{InstancesBuffer3d, ObjectData3d};

use crate::material::{Compiled, PARAMS_GROUP};
use crate::probe::Probe;

/// The most lights one frame sends. Matches `MAX_LIGHTS` in
/// `shaders/mesh.wesl`; the two must move together.
const MAX_LIGHTS: usize = 16;

/// Matches `Light` in `shaders/mesh.wesl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Default)]
struct GpuLight {
    position_kind: [f32; 4],
    direction_radius: [f32; 4],
    color_intensity: [f32; 4],
    cone: [f32; 4],
}

/// Matches `FrameUniforms` in `shaders/mesh.wesl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub(crate) struct FrameUniforms {
    view: [[f32; 4]; 4],
    proj: [[f32; 4]; 4],
    eye_clock: [f32; 4],
    ambient_count: [f32; 4],
    fog_color: [f32; 4],
    fog: [f32; 4],
    lights: [GpuLight; MAX_LIGHTS],
}

/// Matches `ObjectUniforms` in `shaders/mesh.wesl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct ObjectUniforms {
    model: [[f32; 4]; 4],
    normal_matrix: [[f32; 4]; 4],
    color: [f32; 4],
}

/// One light as the shader reads it.
fn gpu_light(light: &kiss3d::light::CollectedLight) -> GpuLight {
    let (kind, radius, cone) = match light.light_type {
        LightType::Directional(_) => (0.0, 0.0, [0.0; 4]),
        LightType::Point { attenuation_radius } => (1.0, attenuation_radius, [0.0; 4]),
        LightType::Spot {
            inner_cone_angle,
            outer_cone_angle,
            attenuation_radius,
        } => (
            2.0,
            attenuation_radius,
            [
                libm::cosf(inner_cone_angle),
                libm::cosf(outer_cone_angle),
                0.0,
                0.0,
            ],
        ),
    };
    let position = light.world_position;
    let direction = light.world_direction.normalize_or_zero();
    GpuLight {
        position_kind: [position.x, position.y, position.z, kind],
        direction_radius: [direction.x, direction.y, direction.z, radius],
        color_intensity: [light.color.x, light.color.y, light.color.z, light.intensity],
        cone,
    }
}

/// The fog row: mode, then start/density, end, and the height falloff.
fn fog_row(fog: &kiss3d::light::Fog) -> [f32; 4] {
    let (mode, a, b) = match fog.mode {
        FogMode::Off => (0.0, 0.0, 0.0),
        FogMode::Linear { start, end } => (1.0, start, end),
        FogMode::Exponential { density } => (2.0, density, 0.0),
        FogMode::ExponentialSquared { density } => (3.0, density, 0.0),
    };
    [mode, a, b, fog.height_falloff]
}

pub(crate) fn frame_uniforms(
    view: &Pose3,
    proj: &Mat4,
    eye: Vec3,
    clock: f32,
    lights: &LightCollection,
) -> FrameUniforms {
    let mut rows = [GpuLight::default(); MAX_LIGHTS];
    for (slot, light) in rows.iter_mut().zip(lights.lights.iter()) {
        *slot = gpu_light(light);
    }
    let live = lights.lights.len().min(MAX_LIGHTS) as f32;
    let ambient = lights.ambient_color;
    FrameUniforms {
        view: view.to_mat4().to_cols_array_2d(),
        proj: proj.to_cols_array_2d(),
        eye_clock: [eye.x, eye.y, eye.z, clock],
        ambient_count: [
            ambient.r * lights.ambient,
            ambient.g * lights.ambient,
            ambient.b * lights.ambient,
            live,
        ],
        fog_color: [
            lights.fog.color.r,
            lights.fog.color.g,
            lights.fog.color.b,
            lights.fog.color.a,
        ],
        fog: fog_row(&lights.fog),
        lights: rows,
    }
}

/// Per-object GPU state: one uniform buffer and the bind groups over it.
struct ShaderGpuData3d {
    object_uniform: wgpu::Buffer,
    object_bind_group: Option<wgpu::BindGroup>,
    texture_bind_group: Option<wgpu::BindGroup>,
    texture_ptr: usize,
}

impl GpuData for ShaderGpuData3d {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// One linked 3D material, shared by every node that names it.
pub(crate) struct ShaderMaterial3d {
    cull: PipelineCache,
    no_cull: PipelineCache,
    object_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    frame_uniform: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    params_bind_group: Option<wgpu::BindGroup>,
    started: Instant,
    frame_counter: Cell<u64>,
    last_frame: Cell<u64>,
}

pub(crate) fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

const fn attribute(shader_location: u32, format: wgpu::VertexFormat) -> wgpu::VertexAttribute {
    wgpu::VertexAttribute {
        offset: 0,
        shader_location,
        format,
    }
}

// Locations 0-2 are `VertexInput` in `shaders/mesh.wesl`.
const POSITION: [wgpu::VertexAttribute; 1] = [attribute(0, wgpu::VertexFormat::Float32x3)];
const NORMAL: [wgpu::VertexAttribute; 1] = [attribute(1, wgpu::VertexFormat::Float32x3)];
const UV: [wgpu::VertexAttribute; 1] = [attribute(2, wgpu::VertexFormat::Float32x2)];

fn vertex_layouts() -> [Option<wgpu::VertexBufferLayout<'static>>; 3] {
    const VEC3: u64 = std::mem::size_of::<[f32; 3]>() as u64;
    const VEC2: u64 = std::mem::size_of::<[f32; 2]>() as u64;
    let layout = |stride: u64, attributes| {
        Some(wgpu::VertexBufferLayout {
            array_stride: stride,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes,
        })
    };
    [
        layout(VEC3, &POSITION),
        layout(VEC3, &NORMAL),
        layout(VEC2, &UV),
    ]
}

/// The frame, object and texture layouts, in the order the pipeline binds
/// them. `shaders/mesh.wesl` declares the matching groups, so `skinned_3d`
/// binds the same three and adds its palette after them.
pub(crate) fn bind_group_layouts() -> [wgpu::BindGroupLayout; 3] {
    let ctxt = Context::get();
    let uniform = |label| {
        ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[uniform_entry(0)],
        })
    };
    [
        uniform("material3d_frame_layout"),
        uniform("material3d_object_layout"),
        ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material3d_texture_layout"),
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
        }),
    ]
}

/// The material's own bind group: its `Params` at binding 0, and a preview's
/// probe at 1 and 2 when the shader carries one.
///
/// `None` when the shader wants neither. A uniform buffer cannot be
/// zero-sized, so a probing shader with no `Params` still gets a placeholder
/// at binding 0 — extra bindings a shader ignores are allowed, a missing one
/// it uses is not.
fn material_group(
    values: &[u8],
    probe: Option<&Probe>,
) -> Option<(wgpu::BindGroupLayout, wgpu::BindGroup)> {
    if values.is_empty() && probe.is_none() {
        return None;
    }
    let ctxt = Context::get();
    let mut layout_entries = vec![uniform_entry(0)];
    if probe.is_some() {
        layout_entries.extend(Probe::layout_entries());
    }
    let layout = ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("material3d_params_layout"),
        entries: &layout_entries,
    });
    let placeholder = [0u8; 16];
    let buffer = ctxt.create_buffer_init(
        Some("material3d_params_uniform"),
        if values.is_empty() {
            &placeholder
        } else {
            values
        },
        wgpu::BufferUsages::UNIFORM,
    );
    let mut entries = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: buffer.as_entire_binding(),
    }];
    if let Some(probe) = probe {
        entries.extend(probe.entries());
    }
    let group = ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("material3d_params_bind_group"),
        layout: &layout,
        entries: &entries,
    });
    Some((layout, group))
}

fn build_pipeline(
    layout: std::rc::Rc<wgpu::PipelineLayout>,
    shader: std::rc::Rc<wgpu::ShaderModule>,
    cull: Option<wgpu::Face>,
    label: &'static str,
) -> PipelineCache {
    PipelineCache::new(move |sample_count| {
        let layouts = vertex_layouts();
        Context::get().create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &layouts,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    // The HDR rasterization target; the resolve pass tonemaps.
                    format: Context::render_format(),
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: cull,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: Context::depth_format(),
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: multisample_state(sample_count),
            multiview_mask: None,
            cache: None,
        })
    })
}

impl ShaderMaterial3d {
    pub(crate) fn new(compiled: &Compiled, probe: Option<&Probe>) -> Self {
        let ctxt = Context::get();
        let [frame_layout, object_layout, texture_layout] = bind_group_layouts();
        let params = material_group(&compiled.params, probe);
        let mut groups = vec![
            Some(&frame_layout),
            Some(&object_layout),
            Some(&texture_layout),
        ];
        if let Some((layout, _)) = params.as_ref() {
            debug_assert_eq!(groups.len() as u32, PARAMS_GROUP);
            groups.push(Some(layout));
        }
        let pipeline_layout = std::rc::Rc::new(ctxt.create_pipeline_layout(
            &wgpu::PipelineLayoutDescriptor {
                label: Some("material3d_pipeline_layout"),
                bind_group_layouts: &groups,
                immediate_size: 0,
            },
        ));
        let shader =
            std::rc::Rc::new(ctxt.create_shader_module(Some("material3d_shader"), &compiled.wgsl));
        let cull = build_pipeline(
            pipeline_layout.clone(),
            shader.clone(),
            Some(wgpu::Face::Back),
            "material3d_pipeline_cull",
        );
        let no_cull = build_pipeline(pipeline_layout, shader, None, "material3d_pipeline_no_cull");
        let frame_uniform = ctxt.create_buffer(&wgpu::BufferDescriptor {
            label: Some("material3d_frame_uniform"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material3d_frame_bind_group"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_uniform.as_entire_binding(),
            }],
        });
        Self {
            cull,
            no_cull,
            object_layout,
            texture_layout,
            frame_uniform,
            frame_bind_group,
            params_bind_group: params.map(|(_, group)| group),
            // The render clock a shader reads, outside the simulation.
            #[allow(clippy::disallowed_methods)]
            started: Instant::now(),
            frame_counter: Cell::new(0),
            last_frame: Cell::new(u64::MAX),
        }
    }

    fn texture_bind_group(&self, texture: &Texture) -> wgpu::BindGroup {
        Context::get().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material3d_texture_bind_group"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&texture.sampler),
                },
            ],
        })
    }
}

impl Material3d for ShaderMaterial3d {
    fn create_gpu_data(&self) -> Box<dyn GpuData> {
        Box::new(ShaderGpuData3d {
            object_uniform: Context::get().create_buffer(&wgpu::BufferDescriptor {
                label: Some("material3d_object_uniform"),
                size: std::mem::size_of::<ObjectUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            object_bind_group: None,
            texture_bind_group: None,
            texture_ptr: 0,
        })
    }

    fn begin_frame(&mut self) {
        self.frame_counter
            .set(self.frame_counter.get().wrapping_add(1));
    }

    fn prepare(
        &mut self,
        pass: usize,
        transform: Pose3,
        scale: Vec3,
        camera: &mut dyn Camera3d,
        lights: &LightCollection,
        data: &ObjectData3d,
        gpu_data: &mut dyn GpuData,
        _viewport_width: u32,
        _viewport_height: u32,
    ) {
        let ctxt = Context::get();
        let gpu_data = gpu_data
            .as_any_mut()
            .downcast_mut::<ShaderGpuData3d>()
            .expect("a material's node carries ShaderGpuData3d");
        let frame = self.frame_counter.get();
        if frame != self.last_frame.get() {
            self.last_frame.set(frame);
            let (view, proj) = camera.view_transform_pair(pass);
            let clock = self.started.elapsed().as_secs_f32();
            let uniforms = frame_uniforms(&view, &proj, camera.eye(), clock, lights);
            ctxt.write_buffer(&self.frame_uniform, 0, bytemuck::bytes_of(&uniforms));
        }
        let model = transform.to_mat4() * Mat4::from_scale(scale);
        let color = data.color();
        ctxt.write_buffer(
            &gpu_data.object_uniform,
            0,
            bytemuck::bytes_of(&ObjectUniforms {
                model: model.to_cols_array_2d(),
                normal_matrix: model.inverse().transpose().to_cols_array_2d(),
                color: [color.r, color.g, color.b, color.a],
            }),
        );
        if gpu_data.object_bind_group.is_none() {
            gpu_data.object_bind_group = Some(ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("material3d_object_bind_group"),
                layout: &self.object_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: gpu_data.object_uniform.as_entire_binding(),
                }],
            }));
        }
        let texture = data.texture();
        let ptr = Arc::as_ptr(texture) as usize;
        if gpu_data.texture_bind_group.is_none() || gpu_data.texture_ptr != ptr {
            gpu_data.texture_bind_group = Some(self.texture_bind_group(texture));
            gpu_data.texture_ptr = ptr;
        }
    }

    fn render(
        &mut self,
        _pass: usize,
        _transform: Pose3,
        _scale: Vec3,
        _camera: &mut dyn Camera3d,
        _lights: &LightCollection,
        data: &ObjectData3d,
        mesh: &mut GpuMesh3d,
        _instances: &mut InstancesBuffer3d,
        gpu_data: &mut dyn GpuData,
        render_pass: &mut wgpu::RenderPass<'_>,
        context: &RenderContext,
    ) {
        if !data.surface_rendering_active() {
            return;
        }
        let gpu_data = gpu_data
            .as_any_mut()
            .downcast_mut::<ShaderGpuData3d>()
            .expect("a material's node carries ShaderGpuData3d");
        mesh.coords()
            .write()
            .expect("kiss3d panicked while writing the mesh's positions")
            .load_to_gpu();
        mesh.normals()
            .write()
            .expect("kiss3d panicked while writing the mesh's normals")
            .load_to_gpu();
        mesh.uvs()
            .write()
            .expect("kiss3d panicked while writing the mesh's UVs")
            .load_to_gpu();
        mesh.faces()
            .write()
            .expect("kiss3d panicked while writing the mesh's faces")
            .load_to_gpu();

        let (
            Some(coords),
            Some(normals),
            Some(uvs),
            Some(faces),
            Some(object_bind_group),
            Some(texture_bind_group),
        ) = (
            mesh.coords_buffer(),
            mesh.normals_buffer(),
            mesh.uvs_buffer(),
            mesh.faces_buffer(),
            gpu_data.object_bind_group.as_ref(),
            gpu_data.texture_bind_group.as_ref(),
        )
        else {
            return;
        };

        let pipeline = if data.backface_culling_enabled() {
            self.cull.get(context.sample_count)
        } else {
            self.no_cull.get(context.sample_count)
        };
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, &self.frame_bind_group, &[]);
        render_pass.set_bind_group(1, object_bind_group, &[]);
        render_pass.set_bind_group(2, texture_bind_group, &[]);
        if let Some(params) = self.params_bind_group.as_ref() {
            render_pass.set_bind_group(PARAMS_GROUP, params, &[]);
        }
        render_pass.set_vertex_buffer(0, coords.slice(..));
        render_pass.set_vertex_buffer(1, normals.slice(..));
        render_pass.set_vertex_buffer(2, uvs.slice(..));
        render_pass.set_index_buffer(faces.slice(..), VERTEX_INDEX_FORMAT);
        render_pass.draw_indexed(0..mesh.num_indices(), 0, 0..1);
    }
}

/// What kiss3d takes on a 3D node.
type Shared3d = std::rc::Rc<std::cell::RefCell<Box<dyn Material3d + 'static>>>;

/// The 3D materials this run has linked, keyed like the 2D cache.
#[derive(Default)]
pub(crate) struct MaterialCache3d {
    linked: std::collections::HashMap<String, Option<Shared3d>>,
    generation: u64,
    /// The channel material and which channel it draws.
    channel: Option<(String, Shared3d)>,
    /// The channel the last frame drew; a change rebuilds every node.
    active: String,
    /// The previewing material's probe, shared with it so the value can be
    /// read without reaching back through `dyn Material3d`.
    probe: Option<std::rc::Rc<Probe>>,
}

impl MaterialCache3d {
    /// Drop everything linked before the last reload, and say whether that
    /// happened.
    pub(crate) fn refresh(&mut self, app: &balaur_core::App) -> bool {
        let now = balaur_core::assets::generation(&app.engine);
        if now == self.generation {
            return false;
        }
        self.generation = now;
        let had = !self.linked.is_empty();
        for reference in self.linked.keys() {
            MaterialManager3d::get_global_manager(|manager| {
                manager.remove(&manager_name(reference));
            });
        }
        self.linked.clear();
        had
    }

    /// Point the previewing material's probe at a pixel and read back what
    /// the frame before wrote there.
    ///
    /// One frame behind, which is what a pointer hovering a viewport wants
    /// anyway: the read stalls until the GPU catches up, so it happens once
    /// per ask and not once per draw.
    pub(crate) fn answer_probe(&self, app: &balaur_core::App) {
        let Some(at) = crate::debug_view::probe_at(&app.engine) else {
            return;
        };
        crate::debug_view::publish_probe(&app.engine, self.probe(at));
    }

    fn probe(&self, at: [f32; 2]) -> Option<[f32; 4]> {
        let probe = self.probe.as_ref()?;
        let read = probe.read();
        probe.aim(at);
        read
    }

    /// Whether the channel view changed since the last frame.
    ///
    /// Turning one on or off changes every node, not only those naming a
    /// material, so the caller rebuilds all of them.
    pub(crate) fn channel_changed(&mut self, channel: &str) -> bool {
        if self.active == channel {
            return false;
        }
        self.active = channel.to_string();
        true
    }

    /// The material a node draws with: the channel while a view is on, its
    /// own otherwise, and none at all — kiss3d's — when it names neither.
    pub(crate) fn for_node(
        &mut self,
        app: &balaur_core::App,
        reference: &str,
        channel: &str,
    ) -> Option<Shared3d> {
        if !channel.is_empty() {
            return self.channel(channel);
        }
        if reference.is_empty() {
            return None;
        }
        self.get(app, reference)
    }

    /// The material that draws `channel`, built on first use.
    pub(crate) fn channel(&mut self, channel: &str) -> Option<Shared3d> {
        if let Some((drawn, material)) = &self.channel {
            if drawn == channel {
                return Some(material.clone());
            }
        }
        let features: Vec<(&str, bool)> = crate::shaders::CHANNELS
            .iter()
            .map(|c| (*c, *c == channel))
            .collect();
        let built = crate::shaders::link(
            &[("package::channel", crate::shaders::CHANNEL)],
            "package::channel",
            &features,
        )
        .map(|unit| {
            ShaderMaterial3d::new(
                &crate::material::Compiled {
                    wgsl: crate::shaders::wgsl(&unit),
                    fields: Vec::new(),
                    params: Vec::new(),
                    probes: false,
                },
                None,
            )
        })
        .inspect_err(|why| tracing::error!(channel, "{why:#}"))
        .ok()?;
        let shared: Shared3d = std::rc::Rc::new(std::cell::RefCell::new(Box::new(built)));
        MaterialManager3d::get_global_manager(|manager| {
            manager.add(shared.clone(), "balaur3d:channel");
        });
        self.channel = Some((channel.to_string(), shared.clone()));
        Some(shared)
    }

    /// The material `reference` names, linking it on first use; `None` for
    /// one that will not link, which leaves kiss3d's own material on the node.
    pub(crate) fn get(&mut self, app: &balaur_core::App, reference: &str) -> Option<Shared3d> {
        if let Some(hit) = self.linked.get(reference) {
            return hit.clone();
        }
        let built = build(app, reference)
            .inspect_err(|why| tracing::error!(material = reference, "{why:#}"))
            .ok()
            .map(|(material, probe)| {
                self.probe = probe;
                let shared: Shared3d = std::rc::Rc::new(std::cell::RefCell::new(
                    Box::new(material) as Box<dyn Material3d>,
                ));
                // Registered, not just attached: `begin_frame` and every
                // per-frame capability the window supplies go through the
                // manager's materials.
                MaterialManager3d::get_global_manager(|manager| {
                    manager.add(shared.clone(), &manager_name(reference));
                });
                shared
            });
        self.linked.insert(reference.to_string(), built.clone());
        built
    }
}

fn manager_name(reference: &str) -> String {
    format!("balaur3d:{reference}")
}

/// A material and, when its shader carries one, the probe it writes into.
fn build(
    app: &balaur_core::App,
    reference: &str,
) -> anyhow::Result<(ShaderMaterial3d, Option<std::rc::Rc<Probe>>)> {
    let asset =
        balaur_core::assets::load_typed::<crate::material::Material>(&app.engine, reference)?;
    let source = crate::material::shader_text(&app.engine, reference, &asset.shader)?;
    let source = crate::preview::requested(&app.engine, &asset.shader, source);
    let modules = crate::shaders::plugin_modules(&app.engine);
    let compiled = crate::material::compile_with(&asset, &source, &modules)?;
    let probe = compiled.probes.then(|| std::rc::Rc::new(Probe::new()));
    let material = ShaderMaterial3d::new(&compiled, probe.as_deref());
    Ok((material, probe))
}
