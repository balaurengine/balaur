//! The 2D light map: the kiss3d half of `light2d` and `occluder2d`.
//!
//! Every frame the scene's lights are drawn additively into an offscreen
//! target the size of the viewport. A shadow-casting light gets a pass of its
//! own: the occluder edges, extruded away from it, are written into a stencil
//! first, and the light draw that follows is rejected wherever they landed.
//! One full-screen node, added last to the 2D scene, then multiplies the
//! frame by the camera's ambient plus what the light map holds — so sprites,
//! polygons and tiles are lit without any of them knowing about it.
//!
//! The multiply lands on everything already drawn, a 3D scene under the 2D
//! one included; debug lines and particles draw after it and stay unlit.
//!
//! Nothing here runs when the scene has no `light2d`: the node is detached
//! and the frame draws exactly as an unlit one does.

use std::cell::RefCell;
use std::rc::Rc;

use bytemuck::{Pod, Zeroable};
use glamx::{Mat3, Pose2, Vec2};
use kiss3d::camera::Camera2d;
use kiss3d::context::Context;
use kiss3d::resource::{
    multisample_state, GpuData, GpuMesh2d, Material2d, PipelineCache, RenderContext2d,
    TextureManager,
};
use kiss3d::scene::{InstancesBuffer2d, Object2d, ObjectData2d, SceneNode2d};

use crate::light::{lights as scene_lights, occluder_edges, shadow_quad, LightKind2d, LitLight2d};
use crate::{shaders, CameraConfig2d};

/// The most lights one frame draws. A shadow-casting light costs a render
/// pass, so a scene that blows past this is told once rather than quietly
/// dropping to single figures of frames per second.
const MAX_LIGHTS: usize = 64;

/// What the frame hands the light map: everything already in world space.
#[derive(Default)]
struct LightScene {
    lights: Vec<LitLight2d>,
    edges: Vec<[Vec2; 2]>,
    ambient: [f32; 3],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct FrameUniforms {
    view: [[f32; 4]; 3],
    proj: [[f32; 4]; 3],
    ambient: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct LightInstance {
    position: [f32; 2],
    radius_kind: [f32; 2],
    color: [f32; 3],
    intensity: f32,
}

fn padded(m: &Mat3) -> [[f32; 4]; 3] {
    let c = m.to_cols_array_2d();
    [
        [c[0][0], c[0][1], c[0][2], 0.0],
        [c[1][0], c[1][1], c[1][2], 0.0],
        [c[2][0], c[2][1], c[2][2], 0.0],
    ]
}

/// The full-screen node that multiplies the frame by the light map, and the
/// scene it is fed each frame. The node exists only while the scene has
/// lights.
pub(crate) struct LightMap {
    scene: Rc<RefCell<LightScene>>,
    node: Option<SceneNode2d>,
}

impl LightMap {
    pub(crate) fn new() -> Self {
        Self {
            scene: Rc::new(RefCell::new(LightScene::default())),
            node: None,
        }
    }

    /// Take the composite out of the 2D scene, before the frame's other 2D
    /// syncs put nodes after it.
    ///
    /// kiss3d removes a child by swapping the last one into its place, so a
    /// node can only be detached without reordering the rest while it is the
    /// last one — which this is, from [`Self::sync`] until anything else is
    /// added. Hence a call at the top of the frame rather than one inside
    /// `sync`.
    pub(crate) fn detach(&mut self) {
        if let Some(node) = &mut self.node {
            node.detach();
        }
    }

    /// Collect this frame's lights and occluders, and put the composite node
    /// back as the last child so it multiplies everything drawn before it.
    ///
    /// Called after every other 2D sync; [`Self::detach`] must have run this
    /// frame, which kiss3d's own `add_child` asserts.
    pub(crate) fn sync(&mut self, app: &balaur_core::App, scene_2d: &mut SceneNode2d) {
        let mut lights = {
            let world = app.engine.world();
            scene_lights(&world, app.engine.root())
        };
        if lights.len() > MAX_LIGHTS {
            warn_once_about_light_count(lights.len());
            lights.truncate(MAX_LIGHTS);
        }
        // The node is left built rather than dropped: a light switched off
        // and on again must not rebuild three pipelines and two textures.
        if lights.is_empty() {
            self.scene.borrow_mut().lights.clear();
            return;
        }
        let casting = lights.iter().any(|light| light.shadows);
        let edges = if casting {
            let world = app.engine.world();
            occluder_edges(&world, app.engine.root())
        } else {
            Vec::new()
        };
        let ambient = app
            .engine
            .try_resource::<CameraConfig2d>()
            .map_or([0.0; 3], |config| config.borrow().ambient);
        *self.scene.borrow_mut() = LightScene {
            lights,
            edges,
            ambient,
        };
        let node = self
            .node
            .get_or_insert_with(|| build_node(Rc::clone(&self.scene)));
        scene_2d.add_child(node.clone());
    }
}

fn warn_once_about_light_count(found: usize) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        tracing::warn!("{found} light2d nodes in the scene; only the first {MAX_LIGHTS} are drawn");
    }
}

/// The composite node: a full-screen draw whose material owns the light map
/// pass. Its mesh and texture are placeholders the material never reads.
fn build_node(scene: Rc<RefCell<LightScene>>) -> SceneNode2d {
    let material: Rc<RefCell<Box<dyn Material2d + 'static>>> =
        Rc::new(RefCell::new(Box::new(LightMapMaterial::new(scene))));
    let placeholder = Rc::new(RefCell::new(GpuMesh2d::new(
        vec![Vec2::ZERO, Vec2::ZERO, Vec2::ZERO],
        vec![[0, 0, 0]],
        None,
        false,
    )));
    let texture = TextureManager::get_global_manager(|tm| tm.get_default());
    let object = Object2d::new(placeholder, 1.0, 1.0, 1.0, texture, material);
    SceneNode2d::new(Vec2::ONE, Pose2::IDENTITY, Some(object))
}

fn linked_shader() -> String {
    shaders::link(
        &[("package::light2d", shaders::LIGHT_2D)],
        "package::light2d",
        &[],
    )
    .map(|linked| shaders::wgsl(&linked))
    .expect("the engine's own shader must link")
}

/// The offscreen target and the stencil it is masked with, rebuilt whenever
/// the viewport changes size.
struct Target {
    width: u32,
    height: u32,
    light: wgpu::TextureView,
    stencil: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

fn build_target(
    width: u32,
    height: u32,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
) -> Target {
    let ctxt = Context::get();
    let size = wgpu::Extent3d {
        width: width.max(1),
        height: height.max(1),
        depth_or_array_layers: 1,
    };
    let texture = ctxt.create_texture(&wgpu::TextureDescriptor {
        label: Some("light_map"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: Context::render_format(),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    // Depth24PlusStencil8 rather than Stencil8: the stencil-only format is
    // optional on Vulkan, and this one is guaranteed everywhere.
    let stencil = ctxt.create_texture(&wgpu::TextureDescriptor {
        label: Some("light_map_stencil"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth24PlusStencil8,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let light = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("light_map_texture_bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&light),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    Target {
        width: size.width,
        height: size.height,
        light,
        stencil: stencil.create_view(&wgpu::TextureViewDescriptor::default()),
        bind_group,
    }
}

/// A vertex or instance buffer that grows to fit what a frame puts in it.
struct GrowingBuffer {
    buffer: wgpu::Buffer,
    capacity: u64,
    label: &'static str,
}

impl GrowingBuffer {
    fn new(label: &'static str, capacity: u64) -> Self {
        Self {
            buffer: Self::allocate(label, capacity),
            capacity,
            label,
        }
    }

    fn allocate(label: &'static str, capacity: u64) -> wgpu::Buffer {
        Context::get().create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: capacity.max(16),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn write(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let needed = bytes.len() as u64;
        if needed > self.capacity {
            self.capacity = needed.next_power_of_two();
            self.buffer = Self::allocate(self.label, self.capacity);
        }
        Context::get().write_buffer(&self.buffer, 0, bytes);
    }
}

struct NoGpuData;

impl GpuData for NoGpuData {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Where one light's shadow triangles sit in the shadow vertex buffer.
struct ShadowRange {
    start: u32,
    count: u32,
}

struct LightMapMaterial {
    scene: Rc<RefCell<LightScene>>,
    light_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    composite_pipeline: PipelineCache,
    frame_uniform: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    target: Option<Target>,
    instances: GrowingBuffer,
    shadows: GrowingBuffer,
    /// One entry per light drawn this frame, in the order they are drawn.
    ranges: Vec<ShadowRange>,
}

fn frame_layout(ctxt: &Context) -> wgpu::BindGroupLayout {
    ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("light_map_frame_layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

fn texture_layout(ctxt: &Context) -> wgpu::BindGroupLayout {
    ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("light_map_texture_layout"),
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
    })
}

/// The stencil state both light-map pipelines share, differing only in what
/// they do with it: shadows write a 1, lights are rejected where one is.
fn stencil_state(write: bool) -> wgpu::DepthStencilState {
    let face = wgpu::StencilFaceState {
        compare: if write {
            wgpu::CompareFunction::Always
        } else {
            wgpu::CompareFunction::NotEqual
        },
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: if write {
            wgpu::StencilOperation::Replace
        } else {
            wgpu::StencilOperation::Keep
        },
    };
    wgpu::DepthStencilState {
        format: wgpu::TextureFormat::Depth24PlusStencil8,
        depth_write_enabled: false,
        depth_compare: wgpu::CompareFunction::Always,
        stencil: wgpu::StencilState {
            front: face,
            back: face,
            read_mask: 0xff,
            write_mask: if write { 0xff } else { 0 },
        },
        bias: wgpu::DepthBiasState::default(),
    }
}

const ADDITIVE: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

/// What the composite draw does to the frame: multiply it, leaving alpha be.
const MULTIPLY: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Dst,
        dst_factor: wgpu::BlendFactor::Zero,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    },
};

const PRIMITIVE: wgpu::PrimitiveState = wgpu::PrimitiveState {
    topology: wgpu::PrimitiveTopology::TriangleList,
    strip_index_format: None,
    front_face: wgpu::FrontFace::Ccw,
    // A shadow quad's winding flips with which side of the edge the light is
    // on, and there is nothing to cull in a full-screen draw.
    cull_mode: None,
    polygon_mode: wgpu::PolygonMode::Fill,
    unclipped_depth: false,
    conservative: false,
};

const LIGHT_ATTRS: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x3, 3 => Float32];
const SHADOW_ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];

const LIGHT_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: std::mem::size_of::<LightInstance>() as wgpu::BufferAddress,
    step_mode: wgpu::VertexStepMode::Instance,
    attributes: &LIGHT_ATTRS,
};

const SHADOW_LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
    array_stride: 8,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &SHADOW_ATTRS,
};

/// The two pipelines that fill the light map. Both draw into a target this
/// module owns, so neither is multisampled.
fn build_offscreen_pipelines(
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
    let ctxt = Context::get();
    let light = ctxt.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("light_map_light_pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_light"),
            buffers: &[LIGHT_LAYOUT],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_light"),
            targets: &[Some(wgpu::ColorTargetState {
                format: Context::render_format(),
                blend: Some(ADDITIVE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: PRIMITIVE,
        depth_stencil: Some(stencil_state(false)),
        multisample: multisample_state(1),
        multiview_mask: None,
        cache: None,
    });
    let shadow = ctxt.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("light_map_shadow_pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_shadow"),
            buffers: &[SHADOW_LAYOUT],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_shadow"),
            targets: &[Some(wgpu::ColorTargetState {
                format: Context::render_format(),
                blend: None,
                write_mask: wgpu::ColorWrites::empty(),
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: PRIMITIVE,
        depth_stencil: Some(stencil_state(true)),
        multisample: multisample_state(1),
        multiview_mask: None,
        cache: None,
    });
    (light, shadow)
}

/// The composite pipeline, rebuilt per sample count by kiss3d's cache: it
/// draws inside the frame's own 2D pass, whose MSAA is not ours to choose.
fn build_composite_pipeline(
    layout: wgpu::PipelineLayout,
    shader: Rc<wgpu::ShaderModule>,
) -> PipelineCache {
    PipelineCache::new(move |sample_count| {
        Context::get().create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("light_map_composite_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_composite"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_composite"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: Context::render_format(),
                    blend: Some(MULTIPLY),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: PRIMITIVE,
            depth_stencil: None,
            multisample: multisample_state(sample_count),
            multiview_mask: None,
            cache: None,
        })
    })
}

impl LightMapMaterial {
    fn new(scene: Rc<RefCell<LightScene>>) -> Self {
        let ctxt = Context::get();
        let frame_layout = frame_layout(&ctxt);
        let texture_layout = texture_layout(&ctxt);
        let offscreen_layout = ctxt.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("light_map_offscreen_layout"),
            bind_group_layouts: &[Some(&frame_layout)],
            immediate_size: 0,
        });
        let composite_layout = ctxt.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("light_map_composite_layout"),
            bind_group_layouts: &[Some(&frame_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let shader = Rc::new(ctxt.create_shader_module(Some("light_map_shader"), &linked_shader()));
        let (light_pipeline, shadow_pipeline) =
            build_offscreen_pipelines(&offscreen_layout, &shader);
        let composite_pipeline = build_composite_pipeline(composite_layout, Rc::clone(&shader));
        let frame_uniform = ctxt.create_buffer(&wgpu::BufferDescriptor {
            label: Some("light_map_frame_uniform"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("light_map_frame_bind_group"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_uniform.as_entire_binding(),
            }],
        });
        Self {
            scene,
            light_pipeline,
            shadow_pipeline,
            composite_pipeline,
            frame_uniform,
            frame_bind_group,
            texture_layout,
            sampler: ctxt.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("light_map_sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            target: None,
            instances: GrowingBuffer::new("light_map_instances", 1024),
            shadows: GrowingBuffer::new("light_map_shadows", 4096),
            ranges: Vec::new(),
        }
    }

    /// The instance for each light, and the shadow triangles each casts,
    /// recorded as the range of the shadow buffer that light owns.
    fn build_geometry(&mut self, scene: &LightScene, far: f32) {
        let mut instances = Vec::with_capacity(scene.lights.len());
        let mut vertices: Vec<[f32; 2]> = Vec::new();
        self.ranges.clear();
        for light in &scene.lights {
            let directional = light.kind == LightKind2d::Directional;
            instances.push(LightInstance {
                position: light.position.to_array(),
                radius_kind: [light.radius, f32::from(u8::from(directional))],
                color: light.color,
                intensity: light.intensity,
            });
            let start = vertices.len() as u32;
            if light.shadows {
                let reach = if directional { far } else { light.radius };
                for edge in &scene.edges {
                    let q = shadow_quad(*edge, light, reach);
                    for corner in [q[0], q[1], q[2], q[0], q[2], q[3]] {
                        vertices.push(corner.to_array());
                    }
                }
            }
            self.ranges.push(ShadowRange {
                start,
                count: vertices.len() as u32 - start,
            });
        }
        self.instances.write(bytemuck::cast_slice(&instances));
        self.shadows.write(bytemuck::cast_slice(&vertices));
    }

    /// Draw the light map: one pass for every light that casts no shadow,
    /// then a pass per light that does, each masked by its own stencil.
    fn encode(&self, target: &Target) {
        let ctxt = Context::get();
        let mut encoder = ctxt.create_command_encoder(Some("light_map"));
        {
            let (color, depth) = attachments(target, true);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("light_map_unshadowed"),
                color_attachments: &[Some(color)],
                depth_stencil_attachment: Some(depth),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_stencil_reference(1);
            for (index, range) in self.ranges.iter().enumerate() {
                if range.count == 0 {
                    self.draw_light(&mut pass, index);
                }
            }
        }
        for (index, range) in self.ranges.iter().enumerate() {
            if range.count == 0 {
                continue;
            }
            let (color, depth) = attachments(target, false);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("light_map_shadowed"),
                color_attachments: &[Some(color)],
                depth_stencil_attachment: Some(depth),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_stencil_reference(1);
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            let bytes = u64::from(range.start) * 8;
            pass.set_vertex_buffer(0, self.shadows.buffer.slice(bytes..));
            pass.draw(0..range.count, 0..1);
            self.draw_light(&mut pass, index);
        }
        ctxt.submit([encoder.finish()]);
    }

    /// One light's quad, read from its own slice of the instance buffer:
    /// every light shares a pipeline, and only the slice picks it out.
    fn draw_light(&self, pass: &mut wgpu::RenderPass<'_>, index: usize) {
        let stride = std::mem::size_of::<LightInstance>() as u64;
        let start = index as u64 * stride;
        pass.set_pipeline(&self.light_pipeline);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.buffer.slice(start..start + stride));
        pass.draw(0..6, 0..1);
    }
}

/// The light map and its stencil as one pass's attachments. The first pass
/// of a frame clears the colour; the per-light passes after it load what the
/// ones before accumulated, and every pass clears the stencil for itself.
fn attachments(
    target: &Target,
    clear_color: bool,
) -> (
    wgpu::RenderPassColorAttachment<'_>,
    wgpu::RenderPassDepthStencilAttachment<'_>,
) {
    (
        wgpu::RenderPassColorAttachment {
            view: &target.light,
            resolve_target: None,
            ops: wgpu::Operations {
                load: if clear_color {
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                } else {
                    wgpu::LoadOp::Load
                },
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        },
        wgpu::RenderPassDepthStencilAttachment {
            view: &target.stencil,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Discard,
            }),
            stencil_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(0),
                store: wgpu::StoreOp::Store,
            }),
        },
    )
}

/// How far a directional light's shadows must reach to leave the view: the
/// visible diagonal, doubled so a caster just off-screen still covers it.
fn view_reach(view: &Mat3, proj: &Mat3) -> f32 {
    let combined = *proj * *view;
    let inverse = combined.inverse();
    if !inverse.is_finite() {
        return 0.0;
    }
    let corner = |x: f32, y: f32| {
        let p = inverse * glamx::Vec3::new(x, y, 1.0);
        Vec2::new(p.x, p.y)
    };
    2.0 * (corner(1.0, 1.0) - corner(-1.0, -1.0)).length()
}

impl Material2d for LightMapMaterial {
    fn create_gpu_data(&self) -> Box<dyn GpuData> {
        Box::new(NoGpuData)
    }

    /// Everything the light map is happens here: kiss3d calls `prepare`
    /// before it submits the frame's own encoder, so a pass submitted from
    /// inside it has already run by the time `render` samples the result.
    fn prepare(
        &mut self,
        _transform: Pose2,
        _scale: Vec2,
        camera: &mut dyn Camera2d,
        _data: &ObjectData2d,
        _mesh: &mut GpuMesh2d,
        _instances: &mut InstancesBuffer2d,
        _gpu_data: &mut dyn GpuData,
        context: &RenderContext2d,
    ) {
        let scene = Rc::clone(&self.scene);
        let scene = scene.borrow();
        if scene.lights.is_empty() {
            return;
        }
        let ctxt = Context::get();
        let (view, proj) = camera.view_transform_pair();
        let [r, g, b] = scene.ambient;
        let frame = FrameUniforms {
            view: padded(&view),
            proj: padded(&proj),
            ambient: [r, g, b, 0.0],
        };
        ctxt.write_buffer(&self.frame_uniform, 0, bytemuck::bytes_of(&frame));
        let width = context.viewport_width.max(1);
        let height = context.viewport_height.max(1);
        if self
            .target
            .as_ref()
            .is_none_or(|target| target.width != width || target.height != height)
        {
            self.target = Some(build_target(
                width,
                height,
                &self.texture_layout,
                &self.sampler,
            ));
        }
        self.build_geometry(&scene, view_reach(&view, &proj));
        if let Some(target) = &self.target {
            self.encode(target);
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
        _gpu_data: &mut dyn GpuData,
        render_pass: &mut wgpu::RenderPass<'_>,
        context: &RenderContext2d,
    ) {
        let Some(target) = &self.target else {
            return;
        };
        if self.scene.borrow().lights.is_empty() {
            return;
        }
        let pipeline = self.composite_pipeline.get(context.sample_count);
        render_pass.set_pipeline(&pipeline);
        render_pass.set_bind_group(0, &self.frame_bind_group, &[]);
        render_pass.set_bind_group(1, &target.bind_group, &[]);
        render_pass.draw(0..6, 0..1);
    }
}
