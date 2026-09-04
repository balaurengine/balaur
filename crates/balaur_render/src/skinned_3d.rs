//! GPU skinning for 3D meshes, as a kiss3d material.
//!
//! The 3D twin of [`crate::skinned_2d`]: joints and weights ride as vertex
//! attributes, the joint palette rides in a uniform, and the vertex shader
//! blends them — so a rig no longer rewrites the mesh's vertex buffers from
//! the CPU every frame. `balaur_core::skeleton::joint_matrices_3d` still
//! computes the palette; only its consumer changed.
//!
//! The CPU path stays as the reference and is still what draws when a node
//! names a `material` asset or a channel view is on: those shaders draw
//! against `package::mesh` and know nothing about a palette.
//!
//! Written against `Material3d` rather than kiss3d's own skinning, whose
//! palette comes from a chain of kiss3d scene nodes; Balaur's bones are ECS
//! entities, and `Skin3d` cannot be built from outside kiss3d anyway.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glamx::{Mat4, Pose3, Vec2, Vec3};
use kiss3d::camera::Camera3d;
use kiss3d::context::Context;
use kiss3d::light::LightCollection;
use kiss3d::resource::vertex_index::VERTEX_INDEX_FORMAT;
use kiss3d::resource::{
    multisample_state, GpuData, GpuMesh3d, Material3d, PipelineCache, RenderContext, Texture,
};
use kiss3d::scene::{InstancesBuffer3d, ObjectData3d, SceneNode3d};

use crate::shader_material_3d::{bind_group_layouts, frame_uniforms, uniform_entry, FrameUniforms};
use crate::shaders;

/// The most bones one mesh may name. 128 `mat4` is 8 KB, which keeps the
/// palette uniform inside the 16 KB every adapter guarantees.
pub(crate) const MAX_JOINTS: usize = 128;

/// The joint palette a skinned mesh reads each frame. Shared between the
/// backend slot, which writes it, and the material, which uploads it.
#[derive(Clone)]
pub(crate) struct SkinHandle3d(Rc<RefCell<Vec<Mat4>>>);

impl SkinHandle3d {
    pub(crate) fn set(&self, palette: Vec<Mat4>) {
        *self.0.borrow_mut() = palette;
    }
}

/// Matches `ObjectUniforms` in `shaders/mesh.wesl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct ObjectUniforms {
    model: [[f32; 4]; 4],
    normal_matrix: [[f32; 4]; 4],
    color: [f32; 4],
}

/// Matches `SkinUniforms` in `shaders/skinned_3d.wesl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct SkinUniforms {
    joints: [[[f32; 4]; 4]; MAX_JOINTS],
}

/// The geometry one skinned mesh draws, as the material's own buffers.
pub(crate) struct SkinnedMesh3d {
    pub(crate) positions: Vec<Vec3>,
    pub(crate) normals: Vec<Vec3>,
    pub(crate) uvs: Vec<Vec2>,
    pub(crate) joints: Vec<[u32; 4]>,
    pub(crate) weights: Vec<[f32; 4]>,
    pub(crate) indices: Vec<[u32; 3]>,
}

fn linked_shader() -> String {
    shaders::link(
        &[("package::skinned_3d", shaders::SKINNED_3D)],
        "package::skinned_3d",
        &[],
    )
    .map(|linked| shaders::wgsl(&linked))
    .expect("the engine's own shader must link")
}

struct Buffers {
    positions: wgpu::Buffer,
    normals: wgpu::Buffer,
    uvs: wgpu::Buffer,
    joints: wgpu::Buffer,
    weights: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    object_uniform: wgpu::Buffer,
    skin_uniform: wgpu::Buffer,
}

/// Point `node` at the skinning material, and hand back the palette the
/// frame writes into.
pub(crate) fn attach(node: &mut SceneNode3d, mesh: &SkinnedMesh3d) -> SkinHandle3d {
    let palette = Rc::new(RefCell::new(Vec::new()));
    let material: Rc<RefCell<Box<dyn Material3d + 'static>>> = Rc::new(RefCell::new(Box::new(
        SkinnedMaterial3d::new(mesh, Rc::clone(&palette)),
    )));
    node.set_material(material);
    SkinHandle3d(palette)
}

fn buffers(mesh: &SkinnedMesh3d) -> Buffers {
    let ctxt = Context::get();
    let count = mesh.positions.len();
    let positions: Vec<[f32; 3]> = mesh.positions.iter().map(Vec3::to_array).collect();
    let normals: Vec<[f32; 3]> = mesh
        .normals
        .iter()
        .map(Vec3::to_array)
        .chain(std::iter::repeat([0.0, 0.0, 1.0]))
        .take(count)
        .collect();
    let uvs: Vec<[f32; 2]> = mesh
        .uvs
        .iter()
        .map(Vec2::to_array)
        .chain(std::iter::repeat([0.0, 0.0]))
        .take(count)
        .collect();
    let indices: Vec<u32> = mesh
        .indices
        .iter()
        .flat_map(|t| t.iter().copied())
        .collect();
    let vertex = |label: &str, bytes: &[u8]| {
        ctxt.create_buffer_init(Some(label), bytes, wgpu::BufferUsages::VERTEX)
    };
    let uniform = |label: &str, size: u64| {
        ctxt.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    };
    Buffers {
        positions: vertex("skinned3d_positions", bytemuck::cast_slice(&positions)),
        normals: vertex("skinned3d_normals", bytemuck::cast_slice(&normals)),
        uvs: vertex("skinned3d_uvs", bytemuck::cast_slice(&uvs)),
        joints: vertex("skinned3d_joints", bytemuck::cast_slice(&mesh.joints)),
        weights: vertex("skinned3d_weights", bytemuck::cast_slice(&mesh.weights)),
        indices: ctxt.create_buffer_init(
            Some("skinned3d_indices"),
            bytemuck::cast_slice(&indices),
            wgpu::BufferUsages::INDEX,
        ),
        index_count: indices.len() as u32,
        object_uniform: uniform(
            "skinned3d_object_uniform",
            std::mem::size_of::<ObjectUniforms>() as u64,
        ),
        skin_uniform: uniform(
            "skinned3d_skin_uniform",
            std::mem::size_of::<SkinUniforms>() as u64,
        ),
    }
}

struct SkinnedGpuData3d {
    object_bind_group: Option<wgpu::BindGroup>,
    texture_bind_group: Option<wgpu::BindGroup>,
    texture_ptr: usize,
}

impl GpuData for SkinnedGpuData3d {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

struct SkinnedMaterial3d {
    pipeline: PipelineCache,
    frame_uniform: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    object_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    skin_bind_group: wgpu::BindGroup,
    buffers: Buffers,
    palette: Rc<RefCell<Vec<Mat4>>>,
}

const fn attribute(location: u32, format: wgpu::VertexFormat) -> wgpu::VertexAttribute {
    wgpu::VertexAttribute {
        offset: 0,
        shader_location: location,
        format,
    }
}

// Locations 0-2 are `package::mesh`'s `VertexInput`; 3 and 4 are the skin.
const POSITION: [wgpu::VertexAttribute; 1] = [attribute(0, wgpu::VertexFormat::Float32x3)];
const NORMAL: [wgpu::VertexAttribute; 1] = [attribute(1, wgpu::VertexFormat::Float32x3)];
const UV: [wgpu::VertexAttribute; 1] = [attribute(2, wgpu::VertexFormat::Float32x2)];
const JOINTS: [wgpu::VertexAttribute; 1] = [attribute(3, wgpu::VertexFormat::Uint32x4)];
const WEIGHTS: [wgpu::VertexAttribute; 1] = [attribute(4, wgpu::VertexFormat::Float32x4)];

fn vertex_layouts() -> [Option<wgpu::VertexBufferLayout<'static>>; 5] {
    let layout = |stride: u64, attributes| {
        Some(wgpu::VertexBufferLayout {
            array_stride: stride,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes,
        })
    };
    [
        layout(12, &POSITION),
        layout(12, &NORMAL),
        layout(8, &UV),
        layout(16, &JOINTS),
        layout(16, &WEIGHTS),
    ]
}

fn build_pipeline(layout: wgpu::PipelineLayout, shader: wgpu::ShaderModule) -> PipelineCache {
    PipelineCache::new(move |sample_count| {
        let layouts = vertex_layouts();
        Context::get().create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skinned3d_pipeline"),
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
                // A rig can turn a triangle inside out; culling would drop it.
                cull_mode: None,
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

impl SkinnedMaterial3d {
    fn new(mesh: &SkinnedMesh3d, palette: Rc<RefCell<Vec<Mat4>>>) -> Self {
        let ctxt = Context::get();
        let [frame_layout, object_layout, texture_layout] = bind_group_layouts();
        let skin_layout = ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skinned3d_skin_layout"),
            entries: &[uniform_entry(0)],
        });
        let pipeline_layout = ctxt.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skinned3d_pipeline_layout"),
            bind_group_layouts: &[
                Some(&frame_layout),
                Some(&object_layout),
                Some(&texture_layout),
                Some(&skin_layout),
            ],
            immediate_size: 0,
        });
        let shader = ctxt.create_shader_module(Some("skinned3d_shader"), &linked_shader());
        let pipeline = build_pipeline(pipeline_layout, shader);
        let buffers = buffers(mesh);
        let frame_uniform = ctxt.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skinned3d_frame_uniform"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = |label: &str, layout, buffer: &wgpu::Buffer| {
            ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            })
        };
        Self {
            frame_bind_group: bind("skinned3d_frame_bind_group", &frame_layout, &frame_uniform),
            skin_bind_group: bind(
                "skinned3d_skin_bind_group",
                &skin_layout,
                &buffers.skin_uniform,
            ),
            pipeline,
            frame_uniform,
            object_layout,
            texture_layout,
            buffers,
            palette,
        }
    }

    /// The palette as the shader reads it: the rig's matrices, then
    /// identities, so a joint the rig never filled leaves its vertex alone —
    /// which is what `skeleton::blend_3d` does with an index it cannot find.
    fn write_skin(&self) {
        let mut joints = [Mat4::IDENTITY.to_cols_array_2d(); MAX_JOINTS];
        for (slot, joint) in self.palette.borrow().iter().take(MAX_JOINTS).enumerate() {
            joints[slot] = joint.to_cols_array_2d();
        }
        Context::get().write_buffer(
            &self.buffers.skin_uniform,
            0,
            bytemuck::bytes_of(&SkinUniforms { joints }),
        );
    }

    fn texture_bind_group(&self, texture: &Texture) -> wgpu::BindGroup {
        Context::get().create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("skinned3d_texture_bind_group"),
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

impl Material3d for SkinnedMaterial3d {
    fn create_gpu_data(&self) -> Box<dyn GpuData> {
        Box::new(SkinnedGpuData3d {
            object_bind_group: None,
            texture_bind_group: None,
            texture_ptr: 0,
        })
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
        let (view, proj) = camera.view_transform_pair(pass);
        // Clock 0: nothing in this shader reads `time()`, and a skinned mesh
        // is posed by the rig rather than by the render clock.
        let frame = frame_uniforms(&view, &proj, camera.eye(), 0.0, lights);
        ctxt.write_buffer(&self.frame_uniform, 0, bytemuck::bytes_of(&frame));
        self.write_skin();
        let model = transform.to_mat4() * Mat4::from_scale(scale);
        let color = data.color();
        ctxt.write_buffer(
            &self.buffers.object_uniform,
            0,
            bytemuck::bytes_of(&ObjectUniforms {
                model: model.to_cols_array_2d(),
                normal_matrix: model.inverse().transpose().to_cols_array_2d(),
                color: [color.r, color.g, color.b, color.a],
            }),
        );
        let gpu_data = gpu_data
            .as_any_mut()
            .downcast_mut::<SkinnedGpuData3d>()
            .expect("the skinning material only ever meets its own gpu data");
        if gpu_data.object_bind_group.is_none() {
            gpu_data.object_bind_group = Some(ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("skinned3d_object_bind_group"),
                layout: &self.object_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.buffers.object_uniform.as_entire_binding(),
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
        _data: &ObjectData3d,
        _mesh: &mut GpuMesh3d,
        _instances: &mut InstancesBuffer3d,
        gpu_data: &mut dyn GpuData,
        render_pass: &mut wgpu::RenderPass<'_>,
        context: &RenderContext,
    ) {
        let gpu_data = gpu_data
            .as_any_mut()
            .downcast_mut::<SkinnedGpuData3d>()
            .expect("the skinning material only ever meets its own gpu data");
        let (Some(object), Some(texture)) = (
            gpu_data.object_bind_group.as_ref(),
            gpu_data.texture_bind_group.as_ref(),
        ) else {
            return;
        };
        let pipeline = self.pipeline.get(context.sample_count);
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, &self.frame_bind_group, &[]);
        render_pass.set_bind_group(1, object, &[]);
        render_pass.set_bind_group(2, texture, &[]);
        render_pass.set_bind_group(3, &self.skin_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.buffers.positions.slice(..));
        render_pass.set_vertex_buffer(1, self.buffers.normals.slice(..));
        render_pass.set_vertex_buffer(2, self.buffers.uvs.slice(..));
        render_pass.set_vertex_buffer(3, self.buffers.joints.slice(..));
        render_pass.set_vertex_buffer(4, self.buffers.weights.slice(..));
        render_pass.set_index_buffer(self.buffers.indices.slice(..), VERTEX_INDEX_FORMAT);
        render_pass.draw_indexed(0..self.buffers.index_count, 0, 0..1);
    }
}
