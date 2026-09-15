//! A project's `material` asset drawn as a kiss3d 3D material.
//!
//! The 3D counterpart of [`crate::shader_material`]: the same four bind groups
//! — frame, object, texture, the material's own values — with the scene's
//! lights and fog folded into the frame one, which is what lets a project's
//! shader light itself without knowing how kiss3d collects them.
//! `shaders/mesh.wesl` is the contract between the two, and
//! [`crate::frame_group`] owns the frame group both this and the skinning
//! material bind.
//!
//! A material draws in the phases its pipelines have targets for: the
//! geometry prepass, the opaque pass, and the refraction pass when its
//! surface is glass. Nothing here draws in the order-independent transparency
//! pass, whose targets it has no pipeline for.

use balaur_core::time::Instant;
use std::any::Any;
use std::cell::Cell;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glamx::{Mat4, Pose3, Vec3};
use kiss3d::camera::Camera3d;
use kiss3d::context::Context;
use kiss3d::light::LightCollection;
use kiss3d::resource::vertex_index::VERTEX_INDEX_FORMAT;
use kiss3d::resource::{
    EnvLight, GpuData, GpuMesh3d, Material3d, MaterialManager3d, PipelineCache, ProbeLighting,
    RenderContext, RenderPhase, Texture,
};
use kiss3d::scene::{InstancesBuffer3d, ObjectData3d};
use kiss3d::wgpu;

use crate::bind_layout::material_group;
use crate::frame_group::FrameGroup;
use crate::material::{Compiled, PARAMS_GROUP};
use crate::probe::Probe;

/// Matches `ObjectUniforms` in `shaders/mesh.wesl`.
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

/// What a node's mirror puts in its object uniform, and the picture the
/// reflection is read from.
///
/// The window renders that picture from a mirrored camera before the frame,
/// so all this reads is where it landed.
fn mirror_of(transform: Pose3, data: &ObjectData3d) -> (ObjectUniforms, Option<wgpu::TextureView>) {
    let mut uniforms = ObjectUniforms {
        model: Mat4::IDENTITY.to_cols_array_2d(),
        normal_matrix: Mat4::IDENTITY.to_cols_array_2d(),
        color: [0.0; 4],
        mirror_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
        mirror: [0.0; 4],
        mirror_normal: [0.0, 1.0, 0.0, 0.0],
    };
    let Some(reflector) = data.reflector() else {
        return (uniforms, None);
    };
    let normal = (transform.rotation * reflector.local_normal()).normalize_or(Vec3::Y);
    uniforms.mirror_view_proj = reflector.view_proj().to_cols_array_2d();
    uniforms.mirror = [reflector.intensity(), 1.0, reflector.normal_falloff(), 0.0];
    uniforms.mirror_normal = [normal.x, normal.y, normal.z, 0.0];
    (uniforms, Some(reflector.color_view().clone()))
}

/// Per-object GPU state: one uniform buffer and the bind groups over it.
struct ShaderGpuData3d {
    object_uniform: wgpu::Buffer,
    object_bind_group: Option<wgpu::BindGroup>,
    texture_bind_group: Option<wgpu::BindGroup>,
    texture_ptr: usize,
    /// Which generation of this node's mirror the object group was built
    /// over. The window remakes the reflection's target on every resize, so
    /// the group has to be built again when the count moves.
    mirror_generation: Option<u64>,
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
    /// The geometry the prepass wants, in the four targets it writes. Built
    /// from the engine's own shader rather than the material's: the prepass
    /// reads geometry, and a material that paints has nothing to add to it.
    prepass: Option<Prepass>,
    object_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    frame: FrameGroup,
    params_bind_group: Option<wgpu::BindGroup>,
    started: Instant,
    frame_counter: Cell<u64>,
    last_frame: Cell<u64>,
    /// One entry per texture slot the material bound, in slot order; a slot
    /// it left out is `None` and gets a one-pixel stand-in.
    slots: Vec<Option<Arc<Texture>>>,
    /// Whether the pipeline carries the per-vertex colour attribute, and so
    /// whether a draw has to bind one.
    vertex_color: bool,
    /// Whether the frame being drawn is a mirror's or a probe's capture. A
    /// mirror must not draw into its own picture, so it stands out of one.
    capturing: Cell<bool>,
}

const fn attribute(shader_location: u32, format: wgpu::VertexFormat) -> wgpu::VertexAttribute {
    wgpu::VertexAttribute {
        offset: 0,
        shader_location,
        format,
    }
}

// Locations 0-2 are `VertexInput` in `shaders/mesh.wesl`, and 3-7 are the
// per-copy half of it, stepped once per instance rather than once per vertex.
const POSITION: [wgpu::VertexAttribute; 1] = [attribute(0, wgpu::VertexFormat::Float32x3)];
const NORMAL: [wgpu::VertexAttribute; 1] = [attribute(1, wgpu::VertexFormat::Float32x3)];
const UV: [wgpu::VertexAttribute; 1] = [attribute(2, wgpu::VertexFormat::Float32x2)];
const COPY_OFFSET: [wgpu::VertexAttribute; 1] = [attribute(3, wgpu::VertexFormat::Float32x3)];
const COPY_COLOR: [wgpu::VertexAttribute; 1] = [attribute(4, wgpu::VertexFormat::Float32x4)];
/// Three columns of one 3x3, laid out back to back for each copy.
const COPY_DEFORM: [wgpu::VertexAttribute; 3] = [
    attribute(5, wgpu::VertexFormat::Float32x3),
    wgpu::VertexAttribute {
        offset: 12,
        shader_location: 6,
        format: wgpu::VertexFormat::Float32x3,
    },
    wgpu::VertexAttribute {
        offset: 24,
        shader_location: 7,
        format: wgpu::VertexFormat::Float32x3,
    },
];

/// One colour per vertex, at the location `shaders/mesh.wesl` guards behind
/// the `vertex_color` feature.
const VERTEX_TINT: [wgpu::VertexAttribute; 1] = [attribute(8, wgpu::VertexFormat::Float32x4)];

fn vertex_layouts(vertex_color: bool) -> Vec<Option<wgpu::VertexBufferLayout<'static>>> {
    const VEC3: u64 = std::mem::size_of::<[f32; 3]>() as u64;
    const VEC2: u64 = std::mem::size_of::<[f32; 2]>() as u64;
    const VEC4: u64 = std::mem::size_of::<[f32; 4]>() as u64;
    let per_vertex = |stride: u64, attributes| {
        Some(wgpu::VertexBufferLayout {
            array_stride: stride,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes,
        })
    };
    let per_copy = |stride: u64, attributes| {
        Some(wgpu::VertexBufferLayout {
            array_stride: stride,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes,
        })
    };
    let mut layouts = vec![
        per_vertex(VEC3, &POSITION),
        per_vertex(VEC3, &NORMAL),
        per_vertex(VEC2, &UV),
        per_copy(VEC3, &COPY_OFFSET),
        per_copy(VEC4, &COPY_COLOR),
        per_copy(3 * VEC3, &COPY_DEFORM),
    ];
    // Last, so a material that did not ask for it leaves every other slot
    // where it was.
    if vertex_color {
        layouts.push(per_vertex(VEC4, &VERTEX_TINT));
    }
    layouts
}

/// The frame, object and texture layouts, in the order the pipeline binds
/// them. `shaders/mesh.wesl` declares the matching groups, so `skinned_3d`
/// binds the same three and adds its palette after them.
/// How many texture slots group 2 binds, matching `TEXTURE_SLOTS` in
/// `material.rs` and the bindings `mesh.wesl` declares.
pub(crate) const TEXTURE_SLOTS: u32 = 6;

pub(crate) fn bind_group_layouts() -> [wgpu::BindGroupLayout; 3] {
    let ctxt = Context::get();
    [
        crate::frame_group::layout(),
        crate::bind_layout::object_layout("material3d_object_layout"),
        crate::bind_layout::sampled_slots_layout(&ctxt, "material3d_texture_layout", TEXTURE_SLOTS),
    ]
}

fn build_pipeline(
    layout: std::rc::Rc<wgpu::PipelineLayout>,
    shader: std::rc::Rc<wgpu::ShaderModule>,
    cull: Option<wgpu::Face>,
    label: &'static str,
    vertex_color: bool,
) -> PipelineCache {
    PipelineCache::new(move |sample_count| {
        crate::pipeline::material_pipeline(
            label,
            &layout,
            &shader,
            &vertex_layouts(vertex_color),
            cull,
            &crate::pipeline::Depth::Tested,
            sample_count,
        )
    })
}

/// The prepass pipelines for a material: the same vertex work, writing the
/// geometry the screen-space passes read instead of a colour.
///
/// One per culling mode, as the colour pass has, because the two have to
/// agree about which faces exist. It shares the material's own pipeline
/// layout so the draw can bind the same groups; the shader uses only the
/// frame's and the object's.
fn build_prepass(
    layout: &std::rc::Rc<wgpu::PipelineLayout>,
    vertex_color: bool,
) -> anyhow::Result<Prepass> {
    let wgsl = crate::shaders::link_prepass(vertex_color)?;
    let shader = std::rc::Rc::new(Context::get().create_shader_module(Some("mesh_prepass"), &wgsl));
    let build = |cull| {
        let (layout, shader) = (layout.clone(), shader.clone());
        PipelineCache::new(move |_| {
            crate::pipeline::prepass_pipeline(&layout, &shader, &vertex_layouts(vertex_color), cull)
        })
    };
    Ok(Prepass {
        cull: build(Some(wgpu::Face::Back)),
        no_cull: build(None),
    })
}

/// The prepass's two pipelines, picked the way the colour pass picks its own.
struct Prepass {
    cull: PipelineCache,
    no_cull: PipelineCache,
}

impl ShaderMaterial3d {
    pub(crate) fn new(compiled: &Compiled, probe: Option<&Probe>) -> Self {
        Self::with_textures(compiled, probe, Vec::new())
    }

    /// `slots` is one entry per [`crate::material::TEXTURE_SLOTS`] name, in
    /// order, `None` for a slot the material left out.
    pub(crate) fn with_textures(
        compiled: &Compiled,
        probe: Option<&Probe>,
        slots: Vec<Option<Arc<Texture>>>,
    ) -> Self {
        let ctxt = Context::get();
        let [frame_layout, object_layout, texture_layout] = bind_group_layouts();
        let params = material_group(&compiled.params, probe, "material3d");
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
            compiled.vertex_color,
        );
        let no_cull = build_pipeline(
            pipeline_layout.clone(),
            shader,
            None,
            "material3d_pipeline_no_cull",
            compiled.vertex_color,
        );
        // A prepass that will not link is a material that contributes no
        // geometry to the screen-space passes, not a material that fails to
        // draw: the engine owns that shader, so a failure here is its own bug.
        let prepass = build_prepass(&pipeline_layout, compiled.vertex_color)
            .inspect_err(|why| tracing::error!("the mesh prepass shader does not link: {why:#}"))
            .ok();
        Self {
            cull,
            no_cull,
            prepass,
            object_layout,
            texture_layout,
            frame: FrameGroup::new(),
            params_bind_group: params.map(|(_, group)| group),
            // The render clock a shader reads, outside the simulation.
            #[allow(clippy::disallowed_methods)]
            started: Instant::now(),
            frame_counter: Cell::new(0),
            last_frame: Cell::new(u64::MAX),
            vertex_color: compiled.vertex_color,
            capturing: Cell::new(false),
            slots,
        }
    }

    /// Whether this node draws in `phase`.
    ///
    /// Glass draws only in the refraction pass, after the opaque scene it
    /// bends has been resolved; the prepass measures opaque geometry, so
    /// neither glass nor a blended surface belongs in it. A blended surface
    /// draws in the opaque pass with alpha blending rather than in the
    /// order-independent one, whose accumulation targets nothing here builds
    /// a pipeline for: a project's material declares one fragment entry
    /// point, and that pass needs a second.
    fn draws_in(&self, phase: RenderPhase, data: &ObjectData3d) -> bool {
        // A mirror renders the scene into its own picture: drawing itself
        // there would sample the texture it is writing, which wgpu refuses.
        if self.capturing.get() && data.reflector().is_some() {
            return false;
        }
        // The fork keeps its own answer to this private, and it is one
        // line: a blended surface is only transparent while its alpha is.
        let blended = matches!(
            data.alpha_mode(),
            kiss3d::scene::AlphaMode::Blend | kiss3d::scene::AlphaMode::Premultiplied
        ) && data.color().a < 1.0;
        let glass = data.transmission() > 0.0;
        match phase {
            RenderPhase::Prepass => !blended && !glass,
            RenderPhase::Opaque => !glass,
            RenderPhase::Transparent => false,
            RenderPhase::Transmission => glass,
        }
    }

    /// Group 2, one texture and sampler per slot. Slot 0 is the node's own
    /// image unless the material named an `albedo` of its own; the rest come
    /// from the material, or from the fallback for the slot.
    fn texture_bind_group(&self, texture: &Texture) -> wgpu::BindGroup {
        // Made here rather than held: a `Texture` reaches the window's own
        // manager when it is dropped, and one kept on a material outlives it.
        let fallbacks = slot_fallbacks();
        let bound: Vec<&Texture> = (0..TEXTURE_SLOTS as usize)
            .map(|slot| match self.slots.get(slot).and_then(Option::as_ref) {
                Some(own) => own.as_ref(),
                None if slot == 0 => texture,
                None => fallbacks[slot].as_ref(),
            })
            .collect();
        crate::bind_layout::sampled_slots_group(
            &Context::get(),
            "material3d_texture_bind_group",
            &self.texture_layout,
            &bound,
        )
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
            mirror_generation: None,
        })
    }

    fn begin_frame(&mut self) {
        self.frame_counter
            .set(self.frame_counter.get().wrapping_add(1));
    }

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
        self.capturing.set(on);
        self.frame.set_capturing(on);
    }

    fn set_clip_plane(&mut self, plane: Option<[f32; 4]>) {
        self.frame.set_clip_plane(plane);
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
        let gpu_data = gpu_data
            .as_any_mut()
            .downcast_mut::<ShaderGpuData3d>()
            .expect("a material's node carries ShaderGpuData3d");
        let frame = self.frame_counter.get();
        if frame != self.last_frame.get() {
            self.last_frame.set(frame);
            let (view, proj) = camera.view_transform_pair(pass);
            let clock = self.started.elapsed().as_secs_f32();
            self.frame.write(
                &view,
                &proj,
                camera.eye(),
                clock,
                lights,
                (viewport_width, viewport_height),
            );
        }
        let model = transform.to_mat4() * Mat4::from_scale(scale);
        let color = data.color();
        let (mut uniforms, mirror) = mirror_of(transform, data);
        uniforms.model = model.to_cols_array_2d();
        uniforms.normal_matrix = model.inverse().transpose().to_cols_array_2d();
        uniforms.color = [color.r, color.g, color.b, color.a];
        ctxt.write_buffer(&gpu_data.object_uniform, 0, bytemuck::bytes_of(&uniforms));
        let generation = data
            .reflector()
            .map(kiss3d::renderer::Reflector::generation);
        if gpu_data.object_bind_group.is_none() || gpu_data.mirror_generation != generation {
            gpu_data.object_bind_group = Some(crate::bind_layout::object_group(
                &self.object_layout,
                &gpu_data.object_uniform,
                mirror.as_ref(),
            ));
            gpu_data.mirror_generation = generation;
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
        instances: &mut InstancesBuffer3d,
        gpu_data: &mut dyn GpuData,
        render_pass: &mut wgpu::RenderPass<'_>,
        context: &RenderContext,
    ) {
        if !data.surface_rendering_active() || !self.draws_in(context.phase, data) {
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
        // The per-copy half. One identity copy when nothing is multiplying
        // this node, which is what makes instancing invisible to a material.
        let copies = instances.len().max(1) as u32;
        instances.positions.load_to_gpu();
        instances.colors.load_to_gpu();
        instances.deformations.load_to_gpu();
        // A material that asked for vertex colours draws a mesh that has
        // none in white: the attribute is filled rather than the draw
        // dropped, so one uncoloured model does not go missing.
        let tints = if self.vertex_color {
            if !mesh.has_colors() {
                mesh.set_colors(Vec::new());
            }
            match mesh.colors_buffer() {
                Some(buffer) => Some(buffer),
                None => return,
            }
        } else {
            None
        };

        let (
            Some(coords),
            Some(normals),
            Some(uvs),
            Some(faces),
            Some(object_bind_group),
            Some(texture_bind_group),
            Some(copy_offsets),
            Some(copy_colors),
            Some(copy_deforms),
        ) = (
            mesh.coords_buffer(),
            mesh.normals_buffer(),
            mesh.uvs_buffer(),
            mesh.faces_buffer(),
            gpu_data.object_bind_group.as_ref(),
            gpu_data.texture_bind_group.as_ref(),
            instances.positions.buffer(),
            instances.colors.buffer(),
            instances.deformations.buffer(),
        )
        else {
            return;
        };

        // The reflector's mirrored projection flips winding, so the pass that
        // draws into it asks for culling off whatever the node said.
        let culls = data.backface_culling_enabled() && !context.force_no_cull;
        let pipeline = match context.phase {
            RenderPhase::Prepass => match self.prepass.as_ref() {
                Some(prepass) if culls => prepass.cull.get(context.sample_count),
                Some(prepass) => prepass.no_cull.get(context.sample_count),
                None => return,
            },
            _ if culls => self.cull.get(context.sample_count),
            _ => self.no_cull.get(context.sample_count),
        };
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, self.frame.group(), &[]);
        render_pass.set_bind_group(1, object_bind_group, &[]);
        render_pass.set_bind_group(2, texture_bind_group, &[]);
        if let Some(params) = self.params_bind_group.as_ref() {
            render_pass.set_bind_group(PARAMS_GROUP, params, &[]);
        }
        render_pass.set_vertex_buffer(0, coords.slice(..));
        render_pass.set_vertex_buffer(1, normals.slice(..));
        render_pass.set_vertex_buffer(2, uvs.slice(..));
        render_pass.set_vertex_buffer(3, copy_offsets.slice(..));
        render_pass.set_vertex_buffer(4, copy_colors.slice(..));
        render_pass.set_vertex_buffer(5, copy_deforms.slice(..));
        if let Some(tints) = tints.as_ref() {
            render_pass.set_vertex_buffer(6, tints.slice(..));
        }
        render_pass.set_index_buffer(faces.slice(..), VERTEX_INDEX_FORMAT);
        render_pass.draw_indexed(0..mesh.num_indices(), 0, 0..copies);
    }
}

/// What kiss3d takes on a 3D node.
type Shared3d = std::rc::Rc<std::cell::RefCell<Box<dyn Material3d + 'static>>>;

crate::material_cache::define!(
    cache = MaterialCache3d,
    shared = Shared3d,
    boxed = Box<dyn Material3d>,
    manager = MaterialManager3d,
    prefix = "balaur3d",
    channel_shader = crate::shaders::CHANNEL,
    channel_material = channel_material,
);

/// The channel view's own material, which takes no params and writes no probe.
/// The one-pixel stand-in for each slot, in slot order: white albedo, a flat
/// normal, non-metallic mid-roughness, no occlusion, black emissive, mid
/// height. The fork owns the pixel values.
pub(crate) fn slot_fallbacks() -> Vec<Arc<Texture>> {
    vec![
        Texture::new_default(),
        Texture::new_default_normal_map(),
        Texture::new_default_metallic_roughness_map(),
        Texture::new_default_ao_map(),
        Texture::new_default_emissive_map(),
        Texture::new_default_height_map(),
    ]
}

fn channel_material(compiled: &crate::material::Compiled) -> ShaderMaterial3d {
    ShaderMaterial3d::new(compiled, None)
}

/// A material and, when its shader carries one, the probe it writes into;
/// `None` for one whose shader draws the other dimension.
fn build(
    app: &balaur_core::App,
    reference: &str,
) -> anyhow::Result<Option<(ShaderMaterial3d, Option<std::rc::Rc<Probe>>)>> {
    let asset =
        balaur_core::assets::load_typed::<crate::material::Material3d>(&app.engine, reference)?;
    let source = crate::material::shader_text(&app.engine, reference, &asset.shader)?;
    let source = crate::preview::requested(&app.engine, &asset.shader, source);
    let modules = crate::shaders::plugin_modules(&app.engine);
    let found = crate::shaders::contract(&source, &modules);
    if !crate::shaders::fits(reference, found, crate::shaders::Contract::Mesh) {
        return Ok(None);
    }
    let compiled = crate::material::compile_with(&asset, &source, &modules)?;
    let probe = compiled.probes.then(|| std::rc::Rc::new(Probe::new()));
    let slots = asset
        .textures()
        .into_iter()
        .map(|path| {
            path.and_then(|path| {
                let path = crate::material::project_path(&app.engine, reference, path)
                    .unwrap_or_else(|| path.to_string());
                crate::texture::upload(&app.engine, &path, crate::texture::PREMULTIPLY_DROPPED)
            })
        })
        .collect();
    let material = ShaderMaterial3d::with_textures(&compiled, probe.as_deref(), slots);
    Ok(Some((material, probe)))
}
