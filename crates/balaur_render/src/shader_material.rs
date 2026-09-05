//! A project's `material` asset drawn as a kiss3d 2D material.
//!
//! The same five vertex buffers and the same frame, object and texture bind
//! groups kiss3d's own `LitMaterial2d` uses, so a material draws the meshes
//! kiss3d built and the sprite path stays single. What differs is group 3 —
//! the material's own values — and the shader, which is whatever the project
//! wrote. `shaders/sprite.wesl` is the contract between the two.

use balaur_core::time::Instant;
use std::any::Any;
use std::cell::Cell;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glamx::{Mat2, Mat3, Pose2, Vec2};
use kiss3d::camera::Camera2d;
use kiss3d::context::Context;
use kiss3d::resource::vertex_index::VERTEX_INDEX_FORMAT;
use kiss3d::resource::{
    GpuData, GpuMesh2d, Material2d, MaterialManager2d, PipelineCache, RenderContext2d, Texture,
    multisample_state,
};
use kiss3d::scene::{InstancesBuffer2d, ObjectData2d};

use crate::material::{Compiled, PARAMS_GROUP};
use crate::probe::Probe;

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
    /// Which screen texture it was built with, for a material that reads one.
    screen_generation: u64,
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
            screen_generation: u64::MAX,
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
    /// The sampler `screen_texture` is read through, for a screen reader.
    screen: Option<wgpu::Sampler>,
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
/// them. `shaders/sprite.wesl` declares the matching groups; a material
/// reading the screen has it at bindings 2 and 3 of the texture group.
fn bind_group_layouts(screen: bool) -> [wgpu::BindGroupLayout; 3] {
    let ctxt = Context::get();
    let uniform = |label| {
        ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(label),
            entries: &[uniform_entry(0)],
        })
    };
    let mut entries = crate::bind_layout::sampled_entries(0).to_vec();
    if screen {
        entries.extend(crate::bind_layout::sampled_entries(2));
    }
    [
        uniform("material_frame_layout"),
        uniform("material_object_layout"),
        ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material_texture_layout"),
            entries: &entries,
        }),
    ]
}

/// The material's own bind group: its `Params` at binding 0, and a preview's
/// probe at 1 and 2 when the shader carries one.
///
/// `None` when it wants neither. A uniform buffer cannot be zero-sized, so a
/// probing shader with no `Params` still gets a placeholder at binding 0.
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
        label: Some("material_params_layout"),
        entries: &layout_entries,
    });
    let placeholder = [0u8; 16];
    let buffer = ctxt.create_buffer_init(
        Some("material_params_uniform"),
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
        label: Some("material_params_bind_group"),
        layout: &layout,
        entries: &entries,
    });
    Some((layout, group))
}

fn build_pipeline(layout: wgpu::PipelineLayout, shader: wgpu::ShaderModule) -> PipelineCache {
    PipelineCache::new(move |sample_count| {
        crate::pipeline::material_pipeline(
            "material_pipeline",
            &layout,
            &shader,
            &vertex_layouts(),
            None,
            &crate::pipeline::Depth::Ignored,
            sample_count,
        )
    })
}

impl ShaderMaterial {
    /// Build the pipeline for one linked material; `reads_screen` binds the
    /// frame so far at the texture group's bindings 2 and 3.
    pub(crate) fn new(compiled: &Compiled, probe: Option<&Probe>, reads_screen: bool) -> Self {
        let ctxt = Context::get();
        let [frame_layout, object_layout, texture_layout] = bind_group_layouts(reads_screen);
        let screen = reads_screen.then(|| {
            ctxt.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("material_screen_sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            })
        });
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
            // The render clock a shader reads, outside the simulation.
            #[allow(clippy::disallowed_methods)]
            started: Instant::now(),
            frame_counter: Cell::new(0),
            last_frame: Cell::new(u64::MAX),
            screen,
        }
    }

    /// `None` for a screen reader the frame has no copy for yet.
    fn texture_bind_group(
        &self,
        texture: &Texture,
        screen: Option<&wgpu::TextureView>,
    ) -> Option<wgpu::BindGroup> {
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&texture.view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&texture.sampler),
            },
        ];
        if let Some(sampler) = &self.screen {
            let view = screen?;
            entries.push(wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(view),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(sampler),
            });
        }
        Some(
            Context::get().create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("material_texture_bind_group"),
                layout: &self.texture_layout,
                entries: &entries,
            }),
        )
    }
}

impl Material2d for ShaderMaterial {
    fn create_gpu_data(&self) -> Box<dyn GpuData> {
        Box::new(ShaderGpuData::new())
    }

    fn reads_screen(&self) -> bool {
        self.screen.is_some()
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
        context: &RenderContext2d,
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
                    clock: [
                        elapsed,
                        context.viewport_width as f32,
                        context.viewport_height as f32,
                        0.0,
                    ],
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
        let screen = if self.screen.is_some() {
            context.screen_generation
        } else {
            0
        };
        if gpu_data.texture_bind_group.is_none()
            || gpu_data.texture_ptr != ptr
            || gpu_data.screen_generation != screen
        {
            gpu_data.texture_bind_group = self.texture_bind_group(texture, context.screen.as_ref());
            gpu_data.texture_ptr = ptr;
            gpu_data.screen_generation = screen;
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

/// What kiss3d takes on a node.
type SharedMaterial = std::rc::Rc<std::cell::RefCell<Box<dyn Material2d + 'static>>>;

crate::material_cache::define!(
    cache = MaterialCache,
    shared = SharedMaterial,
    boxed = Box<dyn Material2d>,
    manager = MaterialManager2d,
    prefix = "balaur",
    channel_shader = crate::shaders::CHANNEL_2D,
    channel_material = channel_material,
);

/// The channel view's own material, which takes no params and writes no probe.
fn channel_material(compiled: &crate::material::Compiled) -> ShaderMaterial {
    ShaderMaterial::new(compiled, None, false)
}

/// A material and, when its shader carries one, the probe it writes into.
fn build(
    app: &balaur_core::App,
    reference: &str,
) -> anyhow::Result<(ShaderMaterial, Option<std::rc::Rc<Probe>>)> {
    let asset =
        balaur_core::assets::load_typed::<crate::material::Material>(&app.engine, reference)?;
    let source = crate::material::shader_text(&app.engine, reference, &asset.shader)?;
    let source = crate::preview::requested(&app.engine, &asset.shader, source);
    let modules = crate::shaders::plugin_modules(&app.engine);
    let compiled = crate::material::compile_with(&asset, &source, &modules)?;
    let probe = compiled.probes.then(|| std::rc::Rc::new(Probe::new()));
    let material = ShaderMaterial::new(&compiled, probe.as_deref(), asset.reads_screen());
    Ok((material, probe))
}
