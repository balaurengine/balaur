//! Group 0 of the 3D contract: the frame's uniforms and the scene-wide
//! textures every 3D material reads.
//!
//! The lights, ambient and fog were always here. What joins them is what the
//! fork hands every registered material once a frame — the sky as an
//! environment map, the occlusion the prepass measured, the reflection probes
//! and the resolved scene glass refracts. WebGPU guarantees four bind groups
//! and the contract spends them on frame, object, textures and params, so
//! these land beside the uniform rather than in a fifth.
//!
//! Each resource is bound whether or not the scene has one: a one-pixel
//! stand-in takes its place, so no pipeline is rebuilt when a sky is loaded
//! and no shader branches on absence.

use bytemuck::{Pod, Zeroable};
use glamx::{Mat4, Pose3, Vec3};
use kiss3d::context::Context;
use kiss3d::light::{FogMode, LightCollection, LightType};
use kiss3d::resource::{EnvLight, ProbeLighting};
use kiss3d::wgpu;

use crate::shaders::{MAX_LIGHTS, MAX_PROBES};

/// Matches `Light` in `shaders/mesh.wesl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Default)]
struct GpuLight {
    position_kind: [f32; 4],
    direction_radius: [f32; 4],
    color_intensity: [f32; 4],
    cone: [f32; 4],
}

/// Matches `Probe` in `shaders/mesh.wesl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Default)]
struct GpuProbe {
    /// xyz the capture point, w whether this slot is live.
    center_live: [f32; 4],
    /// xyz the influence box's low corner, w which array layer holds it.
    low_layer: [f32; 4],
    /// xyz the box's high corner, w the brightness multiplier.
    high_intensity: [f32; 4],
    /// Turn about y, the soft edge's width, the coarsest mip, spare.
    params: [f32; 4],
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
    /// Whether a sky is bound, its coarsest mip, its brightness, its turn.
    environment: [f32; 4],
    /// The viewport in pixels, how many probes are live, and whether the
    /// resolved scene behind glass is bound.
    screen_probes: [f32; 4],
    /// A world plane; a point behind it is dropped. All zero draws everything.
    clip_plane: [f32; 4],
    lights: [GpuLight; MAX_LIGHTS],
    probes: [GpuProbe; MAX_PROBES],
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

/// What the scene hands a material for one frame, beside the lights: the sky,
/// the occlusion, the probes and the picture behind glass.
///
/// Each is `None` until the window supplies it, which it does once a frame for
/// every registered material.
#[derive(Default)]
struct Supplied {
    environment: Option<(wgpu::TextureView, wgpu::Sampler, [f32; 4])>,
    occlusion: Option<wgpu::TextureView>,
    probes: Option<(wgpu::TextureView, Vec<GpuProbe>)>,
    behind: Option<wgpu::TextureView>,
}

/// Group 0's layout, its uniform buffer, and the bind group over both.
///
/// Held by every material that draws the 3D contract, which is what lets
/// `shaders/mesh.wesl` declare one set of bindings for all of them.
pub(crate) struct FrameGroup {
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
    fallback: Fallbacks,
    supplied: Supplied,
    /// Whether what is bound still matches what was supplied. Rebuilding a
    /// bind group every frame costs more than the flag that says not to.
    stale: bool,
    /// The plane a mirror's own pass clips against: what is behind the mirror
    /// is not in front of it, and drawing it would put the room's far wall in
    /// the reflection.
    clip_plane: [f32; 4],
    /// Whether the frame being drawn is a mirror's or a probe's rather than
    /// the camera's. A capture must not sample what it is writing, so no
    /// probe speaks during one.
    capturing: bool,
}

/// The one-pixel stand-ins bound where the scene supplied nothing: a black
/// sky, no occlusion, no probes, and nothing behind the glass.
struct Fallbacks {
    environment: wgpu::TextureView,
    sampler: wgpu::Sampler,
    occlusion: wgpu::TextureView,
    probes: wgpu::TextureView,
    behind: wgpu::TextureView,
    behind_sampler: wgpu::Sampler,
    /// Held so the views above stay valid; nothing reads them.
    _textures: Vec<wgpu::Texture>,
}

/// A one-texel texture of `format`, written with `texel`.
fn one_pixel(label: &'static str, format: wgpu::TextureFormat, texel: &[u8]) -> wgpu::Texture {
    let ctxt = Context::get();
    let size = wgpu::Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
    };
    let texture = ctxt.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    ctxt.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        texel,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(texel.len() as u32),
            rows_per_image: Some(1),
        },
        size,
    );
    texture
}

impl Fallbacks {
    fn new() -> Self {
        let ctxt = Context::get();
        // The formats are the fork's own for each resource, so a real view
        // and its stand-in are interchangeable in one bind group layout.
        let environment = one_pixel(
            "mesh_environment_fallback",
            wgpu::TextureFormat::Rgba16Float,
            &[0u8; 8],
        );
        let occlusion = one_pixel(
            "mesh_occlusion_fallback",
            wgpu::TextureFormat::R16Float,
            // f16 one: nothing is occluded.
            &0x3c00u16.to_le_bytes(),
        );
        let probes = one_pixel(
            "mesh_probe_fallback",
            wgpu::TextureFormat::Rgba16Float,
            &[0u8; 8],
        );
        let behind = one_pixel(
            "mesh_behind_fallback",
            wgpu::TextureFormat::Rgba16Float,
            &[0u8; 8],
        );
        let trilinear = |label: &'static str| {
            ctxt.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            })
        };
        Self {
            environment: environment.create_view(&wgpu::TextureViewDescriptor::default()),
            sampler: trilinear("mesh_environment_fallback_sampler"),
            occlusion: occlusion.create_view(&wgpu::TextureViewDescriptor::default()),
            probes: probes.create_view(&wgpu::TextureViewDescriptor {
                label: Some("mesh_probe_fallback_view"),
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            }),
            behind: behind.create_view(&wgpu::TextureViewDescriptor::default()),
            behind_sampler: trilinear("mesh_behind_fallback_sampler"),
            _textures: vec![environment, occlusion, probes, behind],
        }
    }
}

/// A texture read by the fragment stage, at `binding`.
fn texture_entry(binding: u32, array: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: if array {
                wgpu::TextureViewDimension::D2Array
            } else {
                wgpu::TextureViewDimension::D2
            },
            multisampled: false,
        },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

/// Group 0's layout: the uniform, then the four scene-wide textures in the
/// order `shaders/mesh.wesl` declares them.
pub(crate) fn layout() -> wgpu::BindGroupLayout {
    Context::get().create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("mesh_frame_layout"),
        entries: &[
            crate::bind_layout::uniform_entry(0),
            texture_entry(1, false),
            sampler_entry(2),
            texture_entry(3, false),
            texture_entry(4, true),
            texture_entry(5, false),
            sampler_entry(6),
        ],
    })
}

impl FrameGroup {
    pub(crate) fn new() -> Self {
        let ctxt = Context::get();
        let layout = layout();
        let uniform = ctxt.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mesh_frame_uniform"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let fallback = Fallbacks::new();
        let supplied = Supplied::default();
        let group = build_group(&layout, &uniform, &fallback, &supplied);
        Self {
            layout,
            uniform,
            group,
            fallback,
            supplied,
            stale: false,
            clip_plane: [0.0; 4],
            capturing: false,
        }
    }

    /// The group to bind at 0, rebuilt when the scene supplied a new view.
    pub(crate) fn group(&mut self) -> &wgpu::BindGroup {
        if self.stale {
            self.group = build_group(&self.layout, &self.uniform, &self.fallback, &self.supplied);
            self.stale = false;
        }
        &self.group
    }

    /// The whole frame, written into the uniform buffer.
    pub(crate) fn write(
        &self,
        view: &Pose3,
        proj: &Mat4,
        eye: Vec3,
        clock: f32,
        lights: &LightCollection,
        viewport: (u32, u32),
    ) {
        let uniforms = self.uniforms(view, proj, eye, clock, lights, viewport);
        Context::get().write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniforms));
    }

    fn uniforms(
        &self,
        view: &Pose3,
        proj: &Mat4,
        eye: Vec3,
        clock: f32,
        lights: &LightCollection,
        viewport: (u32, u32),
    ) -> FrameUniforms {
        let mut rows = [GpuLight::default(); MAX_LIGHTS];
        for (slot, light) in rows.iter_mut().zip(lights.lights.iter()) {
            *slot = gpu_light(light);
        }
        let live = lights.lights.len().min(MAX_LIGHTS) as f32;
        let ambient = lights.ambient_color;
        let mut probes = [GpuProbe::default(); MAX_PROBES];
        // A capture renders the scene into a probe's own map; a surface
        // reading a probe there would be reading what is being written.
        let probe_count = match self.supplied.probes.as_ref() {
            Some((_, records)) if !self.capturing => {
                for (slot, record) in probes.iter_mut().zip(records) {
                    *slot = *record;
                }
                records.len().min(MAX_PROBES) as f32
            }
            _ => 0.0,
        };
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
            environment: self
                .supplied
                .environment
                .as_ref()
                .map_or([0.0; 4], |(_, _, row)| *row),
            screen_probes: [
                viewport.0 as f32,
                viewport.1 as f32,
                probe_count,
                f32::from(u8::from(self.supplied.behind.is_some())),
            ],
            clip_plane: self.clip_plane,
            lights: rows,
            probes,
        }
    }

    /// The plane this frame clips against, or none to draw everything.
    pub(crate) fn set_clip_plane(&mut self, plane: Option<[f32; 4]>) {
        self.clip_plane = plane.unwrap_or([0.0; 4]);
    }

    /// Whether the frame being drawn is a capture rather than the camera's.
    pub(crate) fn set_capturing(&mut self, on: bool) {
        self.capturing = on;
    }

    pub(crate) fn set_environment(&mut self, env: Option<EnvLight<'_>>) {
        let next = env.map(|env| {
            (
                env.view.clone(),
                env.sampler.clone(),
                // The coarsest mip is the roughest prefilter, which is what
                // stands in for irradiance.
                [
                    1.0,
                    (env.mip_count.max(1) - 1) as f32,
                    env.intensity,
                    env.rotation,
                ],
            )
        });
        self.stale |= rebinds(self.supplied.environment.is_some(), next.is_some());
        self.supplied.environment = next;
    }

    pub(crate) fn set_occlusion(&mut self, ao: Option<&wgpu::TextureView>) {
        self.stale |= rebinds(self.supplied.occlusion.is_some(), ao.is_some());
        self.supplied.occlusion = ao.cloned();
    }

    pub(crate) fn set_behind(&mut self, behind: Option<&wgpu::TextureView>) {
        self.stale |= rebinds(self.supplied.behind.is_some(), behind.is_some());
        self.supplied.behind = behind.cloned();
        // The window binds this between `prepare` and the refraction pass, so
        // the flag the shader reads is patched in rather than waiting for the
        // next frame's uniform: glass would spend its first frame black.
        let live = f32::from(u8::from(self.supplied.behind.is_some()));
        let offset = std::mem::offset_of!(FrameUniforms, screen_probes) + 3 * size_of::<f32>();
        Context::get().write_buffer(&self.uniform, offset as u64, &live.to_le_bytes());
    }

    pub(crate) fn set_probes(&mut self, probes: Option<ProbeLighting<'_>>) {
        let next = probes.filter(|p| !p.probes.is_empty()).map(|p| {
            let records = p
                .probes
                .iter()
                .take(MAX_PROBES)
                .map(|probe| GpuProbe {
                    center_live: [probe.center.x, probe.center.y, probe.center.z, 1.0],
                    low_layer: [
                        probe.center.x - probe.half_extents.x,
                        probe.center.y - probe.half_extents.y,
                        probe.center.z - probe.half_extents.z,
                        probe.layer as f32,
                    ],
                    high_intensity: [
                        probe.center.x + probe.half_extents.x,
                        probe.center.y + probe.half_extents.y,
                        probe.center.z + probe.half_extents.z,
                        probe.intensity,
                    ],
                    // A zero-width soft edge would divide the ramp by nothing.
                    params: [probe.rotation, probe.falloff.max(1e-4), p.max_lod, 0.0],
                })
                .collect::<Vec<_>>();
            (p.array_view.clone(), records)
        });
        self.stale |= rebinds(self.supplied.probes.is_some(), next.is_some());
        self.supplied.probes = next;
    }
}

/// Whether the group has to be built again, given what was bound and what is
/// being supplied now.
///
/// A supplied resource always rebinds. The window recreates these textures on
/// resize, on a reloaded sky and on a probe capture, and a fresh view can sit
/// at the address a freed one just left, so nothing about the old view proves
/// it is still the same texture.
const fn rebinds(was_real: bool, is_real: bool) -> bool {
    is_real || was_real
}

fn build_group(
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    fallback: &Fallbacks,
    supplied: &Supplied,
) -> wgpu::BindGroup {
    let (environment, sampler) = supplied.environment.as_ref().map_or(
        (&fallback.environment, &fallback.sampler),
        |(view, s, _)| (view, s),
    );
    let occlusion = supplied.occlusion.as_ref().unwrap_or(&fallback.occlusion);
    let probes = supplied
        .probes
        .as_ref()
        .map_or(&fallback.probes, |(view, _)| view);
    let behind = supplied.behind.as_ref().unwrap_or(&fallback.behind);
    Context::get().create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mesh_frame_bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(environment),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(occlusion),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(probes),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(behind),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::Sampler(&fallback.behind_sampler),
            },
        ],
    })
}
