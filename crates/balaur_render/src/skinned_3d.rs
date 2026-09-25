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
    EnvLight, GpuData, GpuMesh3d, Material3d, PipelineCache, ProbeLighting, RenderContext,
    RenderPhase, Texture,
};
use kiss3d::scene::{InstancesBuffer3d, ObjectData3d, SceneNode3d};
use kiss3d::wgpu;

use crate::bind_layout::uniform_entry;
use crate::frame_group::FrameGroup;
use crate::shader_material_3d::bind_group_layouts;
use crate::shaders;

use crate::shaders::MAX_JOINTS;

/// The joint palette a skinned mesh reads each frame. Shared between the
/// backend slot, which writes it, and the material, which uploads it.
#[derive(Clone)]
pub(crate) struct SkinHandle3d(Rc<RefCell<Vec<Mat4>>>);

impl SkinHandle3d {
    pub(crate) fn set(&self, palette: Vec<Mat4>) {
        *self.0.borrow_mut() = palette;
    }
}

/// Matches `ObjectUniforms` in `shaders/mesh.wesl`. The mirror rows are
/// there because the contract declares them; a skinned mesh never is one.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct ObjectUniforms {
    model: [[f32; 4]; 4],
    normal_matrix: [[f32; 4]; 4],
    color: [f32; 4],
    mirror_view_proj: [[f32; 4]; 4],
    mirror: [f32; 4],
    mirror_normal: [f32; 4],
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
    .and_then(|linked| shaders::wgsl(&linked))
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
    frame: FrameGroup,
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
        // No culling: a rig can turn a triangle inside out, and culling
        // would drop it.
        crate::pipeline::material_pipeline(
            "skinned3d_pipeline",
            &layout,
            &shader,
            &vertex_layouts(),
            None,
            &crate::pipeline::Depth::Tested,
            sample_count,
        )
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
        Self {
            skin_bind_group: ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("skinned3d_skin_bind_group"),
                layout: &skin_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffers.skin_uniform.as_entire_binding(),
                }],
            }),
            pipeline,
            frame: FrameGroup::new(),
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

    /// Group 2, the same six slots every shader importing `package::mesh`
    /// reads: the mesh's own image as albedo, and the slot's stand-in for the
    /// five a skinned mesh names no texture for.
    fn texture_bind_group(&self, texture: &Texture) -> wgpu::BindGroup {
        let fallbacks = crate::shader_material_3d::slot_fallbacks();
        let mut bound: Vec<&Texture> = vec![texture];
        bound.extend(fallbacks.iter().skip(1).map(std::convert::AsRef::as_ref));
        crate::bind_layout::sampled_slots_group(
            &Context::get(),
            "skinned3d_texture_bind_group",
            &self.texture_layout,
            &bound,
        )
    }
}

impl Material3d for SkinnedMaterial3d {
    fn set_environment_lighting(&mut self, env: Option<EnvLight<'_>>) {
        self.frame.set_environment(env);
    }

    fn set_ssao(&mut self, ao: Option<&wgpu::TextureView>) {
        self.frame.set_occlusion(ao);
    }

    fn set_reflection_probes(&mut self, probes: Option<ProbeLighting<'_>>) {
        self.frame.set_probes(probes);
    }

    fn set_transmission_background(&mut self, behind: Option<&wgpu::TextureView>) {
        self.frame.set_behind(behind);
    }

    fn set_capture_mode(&mut self, on: bool) {
        self.frame.set_capturing(on);
    }

    fn set_clip_plane(&mut self, plane: Option<[f32; 4]>) {
        self.frame.set_clip_plane(plane);
    }

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
        viewport_width: u32,
        viewport_height: u32,
    ) {
        let ctxt = Context::get();
        let (view, proj) = camera.view_transform_pair(pass);
        // Clock 0: nothing in this shader reads `time()`, and a skinned mesh
        // is posed by the rig rather than by the render clock.
        self.frame.write(
            &view,
            &proj,
            camera.eye(),
            0.0,
            lights,
            (viewport_width, viewport_height),
        );
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
                mirror_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
                mirror: [0.0; 4],
                mirror_normal: [0.0, 1.0, 0.0, 0.0],
            }),
        );
        let gpu_data = gpu_data
            .as_any_mut()
            .downcast_mut::<SkinnedGpuData3d>()
            .expect("the skinning material only ever meets its own gpu data");
        if gpu_data.object_bind_group.is_none() {
            // No mirror: a skinned mesh reflects nothing, so the group takes
            // the stand-in the contract's own group does.
            gpu_data.object_bind_group = Some(crate::bind_layout::object_group(
                &self.object_layout,
                &self.buffers.object_uniform,
                None,
            ));
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
        // No prepass or refraction pipeline here: a skinned mesh shows in the
        // picture, and contributes no geometry to the screen-space passes.
        if context.phase != RenderPhase::Opaque {
            return;
        }
        let pipeline = self.pipeline.get(context.sample_count);
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, self.frame.group(), &[]);
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

// The physics plugin writes a `SolvedMesh` on a node holding a soft body;
// these two read it into the node's vertex buffers each frame.

/// Whether a solver owns this node's vertices, which makes its buffers
/// dynamic and its normals this frame's rather than the asset's.
pub(crate) fn solver_present(
    world: &balaur_core::hecs::World,
    entity: balaur_core::hecs::Entity,
) -> bool {
    world
        .get::<&balaur_core::mesh::SolvedMesh>(entity)
        .is_ok_and(|s| !s.positions.is_empty())
}

/// Draw a node from what the physics solver produced this step: the vertex
/// positions always, the triangles only when the topology changed, which is
/// what a tear does.
///
/// Answers the topology now on the node, for the next frame to compare.
pub(crate) fn draw_solved(
    world: &balaur_core::hecs::World,
    entity: balaur_core::hecs::Entity,
    uploaded: Option<u32>,
    node: &mut SceneNode3d,
) -> Option<u32> {
    let solved = world.get::<&balaur_core::mesh::SolvedMesh>(entity).ok()?;
    if solved.positions.is_empty() {
        return uploaded;
    }
    if uploaded != Some(solved.topology) {
        let faces: Vec<[u32; 3]> = solved.indices.clone();
        node.modify_faces(&mut |fs: &mut Vec<[u32; 3]>| {
            fs.clone_from(&faces);
        });
    }
    let positions: Vec<Vec3> = solved
        .positions
        .iter()
        .map(|p| Vec3::from_array(*p))
        .collect();
    node.modify_vertices(&mut |coords: &mut Vec<Vec3>| {
        coords.clone_from(&positions);
    });
    // The asset's normals belong to the shape as authored; a deformed one has
    // to work its own out.
    node.recompute_normals();
    Some(solved.topology)
}
