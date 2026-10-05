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
use kiss3d::resource::{EnvLight, ProbeLighting, ShadowResources};
use kiss3d::wgpu;

use crate::shaders::{MAX_LIGHTS, MAX_PROBES, MAX_SHADOW_LIGHTS, MAX_SHADOW_VIEWS};

/// Matches `Light` in `shaders/mesh.wesl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Default)]
struct GpuLight {
    position_kind: [f32; 4],
    direction_radius: [f32; 4],
    color_intensity: [f32; 4],
    cone: [f32; 4],
    /// The light layers it lights, then its row in the fork's shadow uniform.
    bits: [u32; 4],
}

/// A light with no shadow row, and one past the primary eight whose row the
/// shader looks up: the markers `NO_SHADOW` and `FIND_SHADOW` in `mesh.wesl`.
const NO_SHADOW: u32 = u32::MAX;
const FIND_SHADOW: u32 = u32::MAX - 1;

/// Matches `Probe` in `shaders/mesh.wesl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Default)]
struct GpuProbe {
    /// xyz the capture point and the box's centre, w whether this slot is live.
    center_live: [f32; 4],
    /// xyz the box's half extents, w which array layer holds it.
    extent_layer: [f32; 4],
    /// The box's turn as a unit quaternion.
    orientation: [f32; 4],
    /// Turn about y, the soft edge's width, the coarsest mip, brightness.
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
    /// The viewport in pixels, how many probes are live, and how many mips the
    /// resolved scene behind glass has: none when it is not bound.
    screen_probes: [f32; 4],
    /// A world plane; a point behind it is dropped. All zero draws everything.
    clip_plane: [f32; 4],
    lights: [GpuLight; MAX_LIGHTS],
    probes: [GpuProbe; MAX_PROBES],
}

/// One light as the shader reads it, with `shadow_row` the row the fork's
/// shadow uniform keeps for it.
fn gpu_light(light: &kiss3d::light::CollectedLight, shadow_row: u32) -> GpuLight {
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
        bits: [light.layers, shadow_row, 0, 0],
    }
}

/// The lights in the order the shader reads them, each with its shadow row.
///
/// The fork's shadow mapper fills rows 0-7 for its primary tier, in the order
/// `split_primary_clustered` picks, so those lights go first and their slot
/// is their row. A light past them has a row only if it casts a shadow, and
/// which one depends on what fit the atlas, so the shader looks it up.
fn ordered_lights(lights: &LightCollection) -> Vec<GpuLight> {
    let (primary, clustered) = lights.split_primary_clustered();
    let primary_rows = primary
        .iter()
        .enumerate()
        .map(|(row, &index)| (index, row as u32));
    let clustered_rows = clustered.iter().map(|&index| {
        let row = if lights.lights[index].casts_shadows {
            FIND_SHADOW
        } else {
            NO_SHADOW
        };
        (index, row)
    });
    primary_rows
        .chain(clustered_rows)
        .take(MAX_LIGHTS)
        .map(|(index, row)| gpu_light(&lights.lights[index], row))
        .collect()
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
    shadow: Option<ShadowResources>,
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
/// sky, no occlusion, no probes, nothing behind the glass, and a shadow
/// uniform that says shadows are off.
struct Fallbacks {
    environment: wgpu::TextureView,
    sampler: wgpu::Sampler,
    occlusion: wgpu::TextureView,
    probes: wgpu::TextureView,
    behind: wgpu::TextureView,
    behind_sampler: wgpu::Sampler,
    shadow: ShadowResources,
    /// Held so the views above stay valid; nothing reads them.
    _textures: Vec<wgpu::Texture>,
}

/// The size of the fork's `ShadowUniforms`, which `mesh.wesl` mirrors: every
/// view's matrix, every light's 32-byte row, then eight floats and a vec4.
pub(crate) const SHADOW_UNIFORM_SIZE: usize =
    MAX_SHADOW_VIEWS * 64 + MAX_SHADOW_LIGHTS * 32 + 8 * 4 + 16;

/// The layers a stand-in behind a `D2Array` binding has. GLES fixes a texture's
/// target when it is made, and with one layer that target is not an array.
const ARRAY_LAYERS: u32 = 2;

/// A one-texel texture of `format` with `layers` layers, each written with `texel`.
fn one_pixel(
    label: &'static str,
    format: wgpu::TextureFormat,
    texel: &[u8],
    layers: u32,
) -> wgpu::Texture {
    let ctxt = Context::get();
    let size = wgpu::Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: layers,
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
        &texel.repeat(layers as usize),
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
            1,
        );
        let occlusion = one_pixel(
            "mesh_occlusion_fallback",
            wgpu::TextureFormat::R16Float,
            // f16 one: nothing is occluded.
            &0x3c00u16.to_le_bytes(),
            1,
        );
        let probes = one_pixel(
            "mesh_probe_fallback",
            wgpu::TextureFormat::Rgba16Float,
            &[0u8; 8],
            ARRAY_LAYERS,
        );
        let behind = one_pixel(
            "mesh_behind_fallback",
            wgpu::TextureFormat::Rgba16Float,
            &[0u8; 8],
            1,
        );
        let shadow_atlas = one_depth_texel();
        let shadow_tint = one_pixel(
            "mesh_shadow_tint_fallback",
            wgpu::TextureFormat::Rgba8Unorm,
            &[255u8; 4],
            ARRAY_LAYERS,
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
            shadow: ShadowResources {
                atlas: shadow_atlas.create_view(&array_view("mesh_shadow_fallback_view")),
                compare_sampler: ctxt.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("mesh_shadow_fallback_sampler"),
                    compare: Some(wgpu::CompareFunction::LessEqual),
                    ..Default::default()
                }),
                // All zero: `shadows_enabled` is off, so nothing samples the atlas.
                uniform: ctxt.create_buffer_init(
                    Some("mesh_shadow_fallback_uniform"),
                    &[0u8; SHADOW_UNIFORM_SIZE],
                    wgpu::BufferUsages::UNIFORM,
                ),
                transmittance: shadow_tint
                    .create_view(&array_view("mesh_shadow_tint_fallback_view")),
                transmittance_sampler: trilinear("mesh_shadow_tint_fallback_sampler"),
            },
            _textures: vec![
                environment,
                occlusion,
                probes,
                behind,
                shadow_atlas,
                shadow_tint,
            ],
        }
    }
}

fn array_view(label: &'static str) -> wgpu::TextureViewDescriptor<'static> {
    wgpu::TextureViewDescriptor {
        label: Some(label),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    }
}

/// A one-texel depth array: what stands in for the shadow atlas. A depth
/// format takes no write, and nothing reads it while shadows are off.
fn one_depth_texel() -> wgpu::Texture {
    Context::get().create_texture(&wgpu::TextureDescriptor {
        label: Some("mesh_shadow_fallback"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: ARRAY_LAYERS,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
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

/// Group 0's layout: the uniform, the four scene-wide textures, then the
/// shadow atlas and its uniform, in the order `shaders/mesh.wesl` declares
/// them.
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
            wgpu::BindGroupLayoutEntry {
                binding: 7,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 8,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                count: None,
            },
            crate::bind_layout::uniform_entry(9),
            texture_entry(10, true),
            sampler_entry(11),
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
        let ordered = ordered_lights(lights);
        for (slot, light) in rows.iter_mut().zip(&ordered) {
            *slot = *light;
        }
        let live = ordered.len() as f32;
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
                self.behind_levels(),
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
        let levels = self.behind_levels();
        let offset = std::mem::offset_of!(FrameUniforms, screen_probes) + 3 * size_of::<f32>();
        Context::get().write_buffer(&self.uniform, offset as u64, &levels.to_le_bytes());
    }

    /// The mips of the scene behind glass, which the shader reads because GLES
    /// cannot ask a texture how many levels it has.
    fn behind_levels(&self) -> f32 {
        self.supplied
            .behind
            .as_ref()
            .map_or(0.0, |view| view.texture().mip_level_count() as f32)
    }

    /// The shadow atlas and uniform this pass reads, which the window hands
    /// every draw. Rebinds only when the mapper made new ones.
    pub(crate) fn set_shadow(&mut self, shadow: Option<&ShadowResources>) {
        let same = match (&self.supplied.shadow, shadow) {
            (Some(was), Some(now)) => {
                was.atlas == now.atlas
                    && was.uniform == now.uniform
                    && was.transmittance == now.transmittance
            }
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.supplied.shadow = shadow.cloned();
            self.stale = true;
        }
    }

    pub(crate) fn set_probes(&mut self, probes: Option<ProbeLighting<'_>>) {
        let next = probes.filter(|p| !p.probes.is_empty()).map(|p| {
            let records = p
                .probes
                .iter()
                .take(MAX_PROBES)
                .map(|probe| {
                    let q = probe.orientation;
                    GpuProbe {
                        center_live: [probe.center.x, probe.center.y, probe.center.z, 1.0],
                        extent_layer: [
                            probe.half_extents.x,
                            probe.half_extents.y,
                            probe.half_extents.z,
                            probe.layer as f32,
                        ],
                        orientation: [q.x, q.y, q.z, q.w],
                        // A zero-width soft edge would divide the ramp by nothing.
                        params: [
                            probe.rotation,
                            probe.falloff.max(1e-4),
                            p.max_lod,
                            probe.intensity,
                        ],
                    }
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
    let shadow = supplied.shadow.as_ref().unwrap_or(&fallback.shadow);
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
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(&shadow.atlas),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::Sampler(&shadow.compare_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: shadow.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: wgpu::BindingResource::TextureView(&shadow.transmittance),
            },
            wgpu::BindGroupEntry {
                binding: 11,
                resource: wgpu::BindingResource::Sampler(&shadow.transmittance_sampler),
            },
        ],
    })
}
