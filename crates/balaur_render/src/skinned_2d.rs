//! GPU skinning for 2D polygons, as a kiss3d material.
//!
//! Each vertex carries up to four joint indices and weights; the vertex
//! shader blends the joint matrices the frame uploaded and the ordinary
//! model, view and projection follow. A vertex whose weights sum to zero is
//! left where it was authored, so an unskinned polygon draws through the
//! same pipeline with an empty palette. The model matrix is built from the
//! scene node's own pose and scale, so a flipped or scaled node flips or
//! scales its polygon the way it does every other shape.
//!
//! Written here against kiss3d's `Material2d` rather than taken from its
//! `SkinnedMesh2d`, which computes its own palette from a bone chain it
//! owns and has no scale in its model transform; Balaur computes the palette
//! from the scene tree and hands the matrices over.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glamx::{Mat3, Pose2, Vec2};
use kiss3d::camera::Camera2d;
use kiss3d::context::Context;
use kiss3d::resource::{
    multisample_state, GpuData, GpuMesh2d, Material2d, PipelineCache, RenderContext2d,
    TextureManager,
};
use kiss3d::scene::{InstancesBuffer2d, Object2d, ObjectData2d, SceneNode2d};

use crate::shaders;
use crate::PolygonMesh;

/// The skinning shader, linked from WESL to WGSL.
///
/// The engine's own shaders are checked by `shaders`' tests, so a failure
/// here is a bug in this build, not in anything a project wrote.
fn linked_shader() -> String {
    shaders::link(
        &[("package::skinned_2d", shaders::SKINNED_2D)],
        "package::skinned_2d",
        &[],
    )
    .map(|linked| shaders::wgsl(&linked))
    .expect("the engine's own shader must link")
}

/// The most bones one polygon may name. Three `vec4` per joint keeps the
/// object uniform well inside the 16 KB every adapter guarantees.
pub(crate) const MAX_JOINTS: usize = 128;

/// The joint palette a skinned polygon reads each frame. Shared between the
/// backend slot, which writes it, and the material, which uploads it.
#[derive(Clone)]
pub(crate) struct SkinHandle(Rc<RefCell<Vec<Mat3>>>);

impl SkinHandle {
    pub(crate) fn set(&self, palette: Vec<Mat3>) {
        *self.0.borrow_mut() = palette;
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct FrameUniforms {
    view: [[f32; 4]; 3],
    proj: [[f32; 4]; 3],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct ObjectUniforms {
    model: [[f32; 4]; 3],
    color: [f32; 4],
    joints: [[f32; 4]; MAX_JOINTS * 3],
}

fn padded(m: &Mat3) -> [[f32; 4]; 3] {
    let c = m.to_cols_array_2d();
    [
        [c[0][0], c[0][1], c[0][2], 0.0],
        [c[1][0], c[1][1], c[1][2], 0.0],
        [c[2][0], c[2][1], c[2][2], 0.0],
    ]
}

/// A polygon's kiss3d node, drawn through the skinning material. The
/// handle comes back only for a mesh with a skin; a rigid polygon has
/// nothing to write per frame.
pub(crate) fn build(
    scene: &mut SceneNode2d,
    polygon: &PolygonMesh,
) -> (SceneNode2d, Option<SkinHandle>) {
    let ctxt = Context::get();
    let count = polygon.positions.len();
    let positions: Vec<[f32; 2]> = polygon.positions.iter().map(Vec2::to_array).collect();
    let uvs: Vec<[f32; 2]> = polygon
        .uvs
        .iter()
        .map(Vec2::to_array)
        .chain(std::iter::repeat([0.0, 0.0]))
        .take(count)
        .collect();
    let (joints, weights): (Vec<[u32; 4]>, Vec<[f32; 4]>) = match &polygon.skin {
        Some(skin) => (skin.joints.clone(), skin.weights.clone()),
        None => (vec![[0; 4]; count], vec![[0.0; 4]; count]),
    };
    let indices: Vec<u32> = polygon
        .indices
        .iter()
        .flat_map(|t| t.iter().copied())
        .collect();
    let buffer = |label: &str, bytes: &[u8], usage: wgpu::BufferUsages| {
        ctxt.create_buffer_init(Some(label), bytes, usage)
    };
    let object_uniform = ctxt.create_buffer(&wgpu::BufferDescriptor {
        label: Some("polygon_object_uniform"),
        size: std::mem::size_of::<ObjectUniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let palette = Rc::new(RefCell::new(Vec::new()));
    let material = SkinnedMaterial::new(
        Buffers {
            positions: buffer(
                "polygon_positions",
                bytemuck::cast_slice(&positions),
                wgpu::BufferUsages::VERTEX,
            ),
            uvs: buffer(
                "polygon_uvs",
                bytemuck::cast_slice(&uvs),
                wgpu::BufferUsages::VERTEX,
            ),
            joints: buffer(
                "polygon_joints",
                bytemuck::cast_slice(&joints),
                wgpu::BufferUsages::VERTEX,
            ),
            weights: buffer(
                "polygon_weights",
                bytemuck::cast_slice(&weights),
                wgpu::BufferUsages::VERTEX,
            ),
            indices: buffer(
                "polygon_indices",
                bytemuck::cast_slice(&indices),
                wgpu::BufferUsages::INDEX,
            ),
            index_count: indices.len() as u32,
            object_uniform,
        },
        Rc::clone(&palette),
    );
    let material: Rc<RefCell<Box<dyn Material2d + 'static>>> =
        Rc::new(RefCell::new(Box::new(material)));
    // The object needs a mesh to exist; the material draws its own buffers
    // and never reads this one.
    let placeholder = Rc::new(RefCell::new(GpuMesh2d::new(
        vec![Vec2::ZERO, Vec2::ZERO, Vec2::ZERO],
        vec![[0, 0, 0]],
        None,
        false,
    )));
    let texture = TextureManager::get_global_manager(|tm| tm.get_default());
    let object = Object2d::new(placeholder, 1.0, 1.0, 1.0, texture, material);
    let node = SceneNode2d::new(Vec2::ONE, Pose2::IDENTITY, Some(object));
    scene.add_child(node.clone());
    let handle = polygon.skin.is_some().then_some(SkinHandle(palette));
    (node, handle)
}

struct Buffers {
    positions: wgpu::Buffer,
    uvs: wgpu::Buffer,
    joints: wgpu::Buffer,
    weights: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    object_uniform: wgpu::Buffer,
}

struct SkinnedGpuData {
    object_bind_group: Option<wgpu::BindGroup>,
    texture_bind_group: Option<wgpu::BindGroup>,
    texture_ptr: usize,
}

impl GpuData for SkinnedGpuData {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

struct SkinnedMaterial {
    pipeline: PipelineCache,
    frame_bind_group: wgpu::BindGroup,
    frame_uniform: wgpu::Buffer,
    object_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    buffers: Buffers,
    palette: Rc<RefCell<Vec<Mat3>>>,
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn vertex_layout(
    stride: usize,
    location: u32,
    format: wgpu::VertexFormat,
) -> wgpu::VertexBufferLayout<'static> {
    // Leaked once per pipeline build: wgpu wants the attribute slice to
    // outlive the descriptor, and four static layouts is what it costs.
    let attributes: &'static [wgpu::VertexAttribute] =
        Box::leak(Box::new([wgpu::VertexAttribute {
            offset: 0,
            shader_location: location,
            format,
        }]));
    wgpu::VertexBufferLayout {
        array_stride: stride as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes,
    }
}

/// The three bind group layouts: per frame, per object, the texture.
fn bind_group_layouts(
    ctxt: &Context,
) -> (
    wgpu::BindGroupLayout,
    wgpu::BindGroupLayout,
    wgpu::BindGroupLayout,
) {
    let frame = ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("polygon_frame_layout"),
        entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX)],
    });
    let object = ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("polygon_object_layout"),
        entries: &[uniform_entry(
            0,
            wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        )],
    });
    let texture = crate::bind_layout::sampled_layout(ctxt, "polygon_texture_layout");
    (frame, object, texture)
}

/// The pipeline, rebuilt per sample count by kiss3d's cache.
fn build_pipeline(
    pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
) -> PipelineCache {
    PipelineCache::new(move |sample_count| {
        let ctxt = Context::get();
        let layouts = [
            Some(vertex_layout(8, 0, wgpu::VertexFormat::Float32x2)),
            Some(vertex_layout(8, 1, wgpu::VertexFormat::Float32x2)),
            Some(vertex_layout(16, 2, wgpu::VertexFormat::Uint32x4)),
            Some(vertex_layout(16, 3, wgpu::VertexFormat::Float32x4)),
        ];
        crate::pipeline::material_pipeline(
            "polygon_pipeline",
            &pipeline_layout,
            &shader,
            &layouts,
            None,
            &crate::pipeline::Depth::Ignored,
            sample_count,
        )
    })
}

impl SkinnedMaterial {
    fn new(buffers: Buffers, palette: Rc<RefCell<Vec<Mat3>>>) -> Self {
        let ctxt = Context::get();
        let (frame_layout, object_layout, texture_layout) = bind_group_layouts(&ctxt);
        let pipeline_layout = ctxt.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("polygon_pipeline_layout"),
            bind_group_layouts: &[
                Some(&frame_layout),
                Some(&object_layout),
                Some(&texture_layout),
            ],
            immediate_size: 0,
        });
        let shader = ctxt.create_shader_module(Some("polygon_shader"), &linked_shader());
        let pipeline = build_pipeline(pipeline_layout, shader);
        let frame_uniform = ctxt.create_buffer(&wgpu::BufferDescriptor {
            label: Some("polygon_frame_uniform"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("polygon_frame_bind_group"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_uniform.as_entire_binding(),
            }],
        });
        Self {
            pipeline,
            frame_bind_group,
            frame_uniform,
            object_layout,
            texture_layout,
            buffers,
            palette,
        }
    }

    /// The per-object uniform: the node's pose and scale as one matrix, its
    /// tint, and the palette, padded to the shader's array with identities.
    fn write_object(&self, transform: Pose2, scale: Vec2, data: &ObjectData2d) {
        let model = transform.to_mat3() * Mat3::from_scale(scale);
        let color = data.color();
        let mut joints = [[0.0f32; 4]; MAX_JOINTS * 3];
        let identity = padded(&Mat3::IDENTITY);
        for slot in 0..MAX_JOINTS {
            joints[slot * 3..slot * 3 + 3].copy_from_slice(&identity);
        }
        for (slot, joint) in self.palette.borrow().iter().take(MAX_JOINTS).enumerate() {
            joints[slot * 3..slot * 3 + 3].copy_from_slice(&padded(joint));
        }
        let uniforms = ObjectUniforms {
            model: padded(&model),
            color: [color.r, color.g, color.b, color.a],
            joints,
        };
        Context::get().write_buffer(
            &self.buffers.object_uniform,
            0,
            bytemuck::bytes_of(&uniforms),
        );
    }
}

impl Material2d for SkinnedMaterial {
    fn create_gpu_data(&self) -> Box<dyn GpuData> {
        Box::new(SkinnedGpuData {
            object_bind_group: None,
            texture_bind_group: None,
            texture_ptr: 0,
        })
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
        let (view, proj) = camera.view_transform_pair();
        let frame = FrameUniforms {
            view: padded(&view),
            proj: padded(&proj),
        };
        ctxt.write_buffer(&self.frame_uniform, 0, bytemuck::bytes_of(&frame));
        self.write_object(transform, scale, data);
        let gpu_data = gpu_data
            .as_any_mut()
            .downcast_mut::<SkinnedGpuData>()
            .expect("the polygon material only ever meets its own gpu data");
        if gpu_data.object_bind_group.is_none() {
            gpu_data.object_bind_group = Some(ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("polygon_object_bind_group"),
                layout: &self.object_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.buffers.object_uniform.as_entire_binding(),
                }],
            }));
        }
        let texture = data.texture();
        let texture_ptr = Arc::as_ptr(texture) as usize;
        if gpu_data.texture_bind_group.is_none() || gpu_data.texture_ptr != texture_ptr {
            gpu_data.texture_bind_group =
                Some(ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("polygon_texture_bind_group"),
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
                }));
            gpu_data.texture_ptr = texture_ptr;
        }
    }

    fn render(
        &mut self,
        _transform: Pose2,
        _scale: Vec2,
        _camera: &mut dyn Camera2d,
        _data: &ObjectData2d,
        _mesh: &mut GpuMesh2d,
        _instances: &mut InstancesBuffer2d,
        gpu_data: &mut dyn GpuData,
        render_pass: &mut wgpu::RenderPass<'_>,
        context: &RenderContext2d,
    ) {
        let gpu_data = gpu_data
            .as_any_mut()
            .downcast_mut::<SkinnedGpuData>()
            .expect("the polygon material only ever meets its own gpu data");
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
        render_pass.set_vertex_buffer(0, self.buffers.positions.slice(..));
        render_pass.set_vertex_buffer(1, self.buffers.uvs.slice(..));
        render_pass.set_vertex_buffer(2, self.buffers.joints.slice(..));
        render_pass.set_vertex_buffer(3, self.buffers.weights.slice(..));
        render_pass.set_index_buffer(self.buffers.indices.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.draw_indexed(0..self.buffers.index_count, 0, 0..1);
    }
}
