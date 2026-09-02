//! A project's `material` asset drawn as a kiss3d 2D material.
//!
//! The same five vertex buffers and the same frame, object and texture bind
//! groups kiss3d's own `LitMaterial2d` uses, so a material draws the meshes
//! kiss3d built and the sprite path stays single. What differs is group 3 —
//! the material's own values — and the shader, which is whatever the project
//! wrote. `shaders/sprite.wesl` is the contract between the two.

use std::any::Any;
use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use glamx::{Mat2, Mat3, Pose2, Vec2};
use kiss3d::camera::Camera2d;
use kiss3d::context::Context;
use kiss3d::resource::vertex_index::VERTEX_INDEX_FORMAT;
use kiss3d::resource::{
    multisample_state, GpuData, GpuMesh2d, Material2d, PipelineCache, RenderContext2d, Texture,
};
use kiss3d::scene::{InstancesBuffer2d, ObjectData2d};

use crate::material::{Compiled, PARAMS_GROUP};

/// Matches `FrameUniforms` in `shaders/sprite.wesl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct FrameUniforms {
    view: [[f32; 4]; 3],
    proj: [[f32; 4]; 3],
    clock: [f32; 4],
}

/// Matches `ObjectUniforms` in `shaders/sprite.wesl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct ObjectUniforms {
    model: [[f32; 4]; 3],
    scale: [[f32; 4]; 2],
    color: [f32; 4],
}

fn padded_mat3(m: &Mat3) -> [[f32; 4]; 3] {
    let c = m.to_cols_array_2d();
    [
        [c[0][0], c[0][1], c[0][2], 0.0],
        [c[1][0], c[1][1], c[1][2], 0.0],
        [c[2][0], c[2][1], c[2][2], 0.0],
    ]
}

fn padded_mat2(m: &Mat2) -> [[f32; 4]; 2] {
    let c = m.to_cols_array_2d();
    [[c[0][0], c[0][1], 0.0, 0.0], [c[1][0], c[1][1], 0.0, 0.0]]
}

/// Per-object GPU state: one uniform buffer and the bind groups over it.
struct ShaderGpuData {
    object_uniform: wgpu::Buffer,
    object_bind_group: Option<wgpu::BindGroup>,
    texture_bind_group: Option<wgpu::BindGroup>,
    /// Which texture the bind group was built for, so it is rebuilt only when
    /// the node's image actually changes.
    texture_ptr: usize,
}

impl ShaderGpuData {
    fn new() -> Self {
        Self {
            object_uniform: Context::get().create_buffer(&wgpu::BufferDescriptor {
                label: Some("material_object_uniform"),
                size: std::mem::size_of::<ObjectUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            object_bind_group: None,
            texture_bind_group: None,
            texture_ptr: 0,
        }
    }
}

impl GpuData for ShaderGpuData {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// One linked material, shared by every node that names it.
pub(crate) struct ShaderMaterial {
    pipeline: PipelineCache,
    object_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    frame_uniform: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    /// The material's own values. `None` when the shader declares no `Params`,
    /// in which case group 3 is not in the pipeline layout either.
    params_bind_group: Option<wgpu::BindGroup>,
    /// Where `time()` counts from. Wall clock, not the tick: a shader is an
    /// observer, so nothing it reads can reach the simulation.
    started: Instant,
    frame_counter: Cell<u64>,
    last_frame: Cell<u64>,
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
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

const fn float32x2(shader_location: u32, offset: u64) -> wgpu::VertexAttribute {
    wgpu::VertexAttribute {
        offset,
        shader_location,
        format: wgpu::VertexFormat::Float32x2,
    }
}

// Locations 0-5 are `VertexInput` in `shaders/sprite.wesl`. Held as consts so
// a layout can borrow them for `'static`.
const POSITION: [wgpu::VertexAttribute; 1] = [float32x2(0, 0)];
const UV: [wgpu::VertexAttribute; 1] = [float32x2(1, 0)];
const INSTANCE_POSITION: [wgpu::VertexAttribute; 1] = [float32x2(2, 0)];
const INSTANCE_COLOR: [wgpu::VertexAttribute; 1] = [wgpu::VertexAttribute {
    offset: 0,
    shader_location: 3,
    format: wgpu::VertexFormat::Float32x4,
}];
const DEFORMATION: [wgpu::VertexAttribute; 2] = [float32x2(4, 0), float32x2(5, 8)];

/// The five buffers kiss3d binds for a 2D object: vertex positions and UVs,
/// then the instance offset, colour and deformation.
fn vertex_layouts() -> [Option<wgpu::VertexBufferLayout<'static>>; 5] {
    const VEC2: u64 = std::mem::size_of::<[f32; 2]>() as u64;
    const VEC4: u64 = std::mem::size_of::<[f32; 4]>() as u64;
    let vertex = |array_stride, attributes| {
        Some(wgpu::VertexBufferLayout {
            array_stride,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes,
        })
    };
    let instance = |array_stride, attributes| {
        Some(wgpu::VertexBufferLayout {
            array_stride,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes,
        })
    };
    [
        vertex(VEC2, &POSITION),
        vertex(VEC2, &UV),
        instance(VEC2, &INSTANCE_POSITION),
        instance(VEC4, &INSTANCE_COLOR),
        instance(VEC4, &DEFORMATION),
    ]
}

/// The frame, object and texture layouts, in the order the pipeline binds
/// them. `shaders/sprite.wesl` declares the matching groups.
fn bind_group_layouts() -> [wgpu::BindGroupLayout; 3] {
    let ctxt = Context::get();
    let uniform = |label| {
        ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[uniform_entry(0)],
        })
    };
    [
        uniform("material_frame_layout"),
        uniform("material_object_layout"),
        ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material_texture_layout"),
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

/// Group 3 and the buffer behind it, for a shader that declares `Params`.
///
/// `None` otherwise: a uniform buffer cannot be zero-sized, and a layout
/// entry with nothing behind it is a validation error.
fn params_group(values: &[u8]) -> Option<(wgpu::BindGroupLayout, wgpu::BindGroup)> {
    if values.is_empty() {
        return None;
    }
    let ctxt = Context::get();
    let layout = ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("material_params_layout"),
        entries: &[uniform_entry(0)],
    });
    let buffer = ctxt.create_buffer_init(
        Some("material_params_uniform"),
        values,
        wgpu::BufferUsages::UNIFORM,
    );
    let group = ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("material_params_bind_group"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    Some((layout, group))
}

fn build_pipeline(layout: wgpu::PipelineLayout, shader: wgpu::ShaderModule) -> PipelineCache {
    PipelineCache::new(move |sample_count| {
        let layouts = vertex_layouts();
        Context::get().create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("material_pipeline"),
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
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: multisample_state(sample_count),
            multiview_mask: None,
            cache: None,
        })
    })
}

impl ShaderMaterial {
    /// Build the pipeline for one linked material.
    pub(crate) fn new(compiled: &Compiled) -> Self {
        let ctxt = Context::get();
        let [frame_layout, object_layout, texture_layout] = bind_group_layouts();
        let params = params_group(&compiled.params);
        let mut groups = vec![
            Some(&frame_layout),
            Some(&object_layout),
            Some(&texture_layout),
        ];
        if let Some((layout, _)) = params.as_ref() {
            debug_assert_eq!(groups.len() as u32, PARAMS_GROUP);
            groups.push(Some(layout));
        }
        let pipeline_layout = ctxt.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("material_pipeline_layout"),
            bind_group_layouts: &groups,
            immediate_size: 0,
        });
        let shader = ctxt.create_shader_module(Some("material_shader"), &compiled.wgsl);
        let pipeline = build_pipeline(pipeline_layout, shader);
        let frame_uniform = ctxt.create_buffer(&wgpu::BufferDescriptor {
            label: Some("material_frame_uniform"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material_frame_bind_group"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_uniform.as_entire_binding(),
            }],
        });
        Self {
            pipeline,
            object_layout,
            texture_layout,
            frame_uniform,
            frame_bind_group,
            params_bind_group: params.map(|(_, group)| group),
            started: Instant::now(),
            frame_counter: Cell::new(0),
            last_frame: Cell::new(u64::MAX),
        }
    }

    fn texture_bind_group(&self, texture: &Texture) -> wgpu::BindGroup {
        Context::get().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material_texture_bind_group"),
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

impl Material2d for ShaderMaterial {
    fn create_gpu_data(&self) -> Box<dyn GpuData> {
        Box::new(ShaderGpuData::new())
    }

    fn begin_frame(&mut self) {
        self.frame_counter
            .set(self.frame_counter.get().wrapping_add(1));
    }

    fn prepare(
        &mut self,
        transform: Pose2,
        scale: Vec2,
        camera: &mut dyn Camera2d,
        data: &ObjectData2d,
        _mesh: &mut GpuMesh2d,
        _instances: &mut InstancesBuffer2d,
        gpu_data: &mut dyn GpuData,
        _context: &RenderContext2d,
    ) {
        let ctxt = Context::get();
        let gpu_data = gpu_data
            .as_any_mut()
            .downcast_mut::<ShaderGpuData>()
            .expect("a material's node carries ShaderGpuData");
        // The camera moves once per frame, not once per object.
        let frame = self.frame_counter.get();
        if frame != self.last_frame.get() {
            self.last_frame.set(frame);
            let (view, proj) = camera.view_transform_pair();
            let elapsed = self.started.elapsed().as_secs_f32();
            ctxt.write_buffer(
                &self.frame_uniform,
                0,
                bytemuck::bytes_of(&FrameUniforms {
                    view: padded_mat3(&view),
                    proj: padded_mat3(&proj),
                    clock: [elapsed, 0.0, 0.0, 0.0],
                }),
            );
        }
        let color = data.color();
        ctxt.write_buffer(
            &gpu_data.object_uniform,
            0,
            bytemuck::bytes_of(&ObjectUniforms {
                model: padded_mat3(&transform.to_mat3()),
                scale: padded_mat2(&Mat2::from_diagonal(scale)),
                color: [color.r, color.g, color.b, color.a],
            }),
        );
        if gpu_data.object_bind_group.is_none() {
            gpu_data.object_bind_group = Some(ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("material_object_bind_group"),
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
        _transform: Pose2,
        _scale: Vec2,
        _camera: &mut dyn Camera2d,
        _data: &ObjectData2d,
        mesh: &mut GpuMesh2d,
        instances: &mut InstancesBuffer2d,
        gpu_data: &mut dyn GpuData,
        render_pass: &mut wgpu::RenderPass<'_>,
        context: &RenderContext2d,
    ) {
        let gpu_data = gpu_data
            .as_any_mut()
            .downcast_mut::<ShaderGpuData>()
            .expect("a material's node carries ShaderGpuData");
        let instance_count = instances.len();
        instances.positions.load_to_gpu();
        instances.colors.load_to_gpu();
        instances.deformations.load_to_gpu();
        mesh.load_to_gpu();

        let coords = mesh
            .coords()
            .read()
            .expect("kiss3d panicked while writing the mesh's positions");
        let uvs = mesh
            .uvs()
            .read()
            .expect("kiss3d panicked while writing the mesh's UVs");
        let faces = mesh
            .faces()
            .read()
            .expect("kiss3d panicked while writing the mesh's faces");
        let (
            Some(positions),
            Some(colors),
            Some(deformations),
            Some(coords),
            Some(uvs),
            Some(faces),
            Some(object_bind_group),
            Some(texture_bind_group),
        ) = (
            instances.positions.buffer(),
            instances.colors.buffer(),
            instances.deformations.buffer(),
            coords.buffer(),
            uvs.buffer(),
            faces.buffer(),
            gpu_data.object_bind_group.as_ref(),
            gpu_data.texture_bind_group.as_ref(),
        )
        else {
            return;
        };

        render_pass.set_pipeline(&self.pipeline.get(context.sample_count));
        render_pass.set_bind_group(0, &self.frame_bind_group, &[]);
        render_pass.set_bind_group(1, object_bind_group, &[]);
        render_pass.set_bind_group(2, texture_bind_group, &[]);
        if let Some(params) = self.params_bind_group.as_ref() {
            render_pass.set_bind_group(PARAMS_GROUP, params, &[]);
        }
        render_pass.set_vertex_buffer(0, coords.slice(..));
        render_pass.set_vertex_buffer(1, uvs.slice(..));
        render_pass.set_vertex_buffer(2, positions.slice(..));
        render_pass.set_vertex_buffer(3, colors.slice(..));
        render_pass.set_vertex_buffer(4, deformations.slice(..));
        render_pass.set_index_buffer(faces.slice(..), VERTEX_INDEX_FORMAT);
        render_pass.draw_indexed(0..mesh.num_indices(), 0, 0..instance_count as u32);
    }
}

/// The materials this run has linked: one pipeline per `material` reference,
/// however many nodes name it.
#[derive(Default)]
pub(crate) struct MaterialCache {
    /// `None` records a material that would not link, so the error is logged
    /// once rather than every frame.
    linked: std::collections::HashMap<String, Option<SharedMaterial>>,
    /// The asset generation these were linked at.
    generation: u64,
}

/// What kiss3d takes on a node.
type SharedMaterial = std::rc::Rc<std::cell::RefCell<Box<dyn Material2d + 'static>>>;

impl MaterialCache {
    /// Drop everything linked before the last reload, and say whether that
    /// happened.
    ///
    /// A caller that answers `true` must also rebuild the nodes drawing with
    /// a material: they hold the old pipeline, and clearing a cache does not
    /// reach into a scene graph.
    pub(crate) fn refresh(&mut self, app: &balaur_core::App) -> bool {
        let now = balaur_core::assets::generation(&app.engine);
        if now == self.generation {
            return false;
        }
        self.generation = now;
        let had = !self.linked.is_empty();
        self.linked.clear();
        had
    }

    /// The material `reference` names, linking it on first use.
    ///
    /// `None` for one that will not link — the node keeps kiss3d's own
    /// material, so a shader with a typo in it costs a log line and a plain
    /// sprite rather than the frame.
    pub(crate) fn get(
        &mut self,
        app: &balaur_core::App,
        reference: &str,
    ) -> Option<SharedMaterial> {
        if let Some(hit) = self.linked.get(reference) {
            return hit.clone();
        }
        let built = build(app, reference)
            .inspect_err(|why| tracing::error!(material = reference, "{why:#}"))
            .ok()
            .map(|material| {
                std::rc::Rc::new(std::cell::RefCell::new(
                    Box::new(material) as Box<dyn Material2d>
                ))
            });
        self.linked.insert(reference.to_string(), built.clone());
        built
    }
}

fn build(app: &balaur_core::App, reference: &str) -> anyhow::Result<ShaderMaterial> {
    let asset =
        balaur_core::assets::load_typed::<crate::material::Material>(&app.engine, reference)?;
    let source = balaur_core::project::scene_text(&app.engine, &asset.shader)?;
    let compiled = crate::material::compile(&asset, &source)?;
    Ok(ShaderMaterial::new(&compiled))
}
