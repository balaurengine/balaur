//! The render pipeline every Balaur material builds.
//!
//! All four — 2D and 3D materials, skinned polygons and skinned meshes —
//! rasterize triangles into the same blended target with the same entry
//! point names; they differ only in their vertex buffers, whether they cull,
//! whether they test depth, and how they blend. Those are the arguments here,
//! and the rest of the descriptor is written once.

use std::rc::Rc;

use kiss3d::context::Context;
use kiss3d::resource::{PipelineCache, multisample_state};
use kiss3d::scene::Blend2d;
use kiss3d::wgpu;

/// How a pipeline blends its colour over the target: straight alpha, which
/// every 3D colour pass takes, or any of the fork's 2D modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Blend {
    Straight,
    /// For a texture whose colour already carries its alpha: blending it
    /// straight multiplies by the alpha twice and draws it dark.
    Premultiplied,
    Mode(Blend2d),
}

impl Blend {
    /// The blend a 2D node asks for, as its `blend_mode` or its texture's
    /// upload set it.
    pub(crate) fn of_2d(blend: Blend2d) -> Self {
        match blend {
            Blend2d::Alpha => Self::Straight,
            Blend2d::PremultipliedAlpha => Self::Premultiplied,
            other => Self::Mode(other),
        }
    }

    /// `None` for an opaque surface, which writes over what is there.
    fn state(self) -> Option<wgpu::BlendState> {
        match self {
            Self::Straight => Some(wgpu::BlendState::ALPHA_BLENDING),
            Self::Premultiplied => Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            Self::Mode(mode) => mode.blend_state(),
        }
    }
}

/// Every 2D blend, in the order [`Pipelines2d`] keeps their pipelines.
const BLENDS_2D: [Blend2d; 6] = [
    Blend2d::Alpha,
    Blend2d::PremultipliedAlpha,
    Blend2d::Additive,
    Blend2d::Multiply,
    Blend2d::Screen,
    Blend2d::Opaque,
];

/// A 2D material's pipelines, one per blend and culling mode, each built at
/// first use.
pub(crate) struct Pipelines2d {
    caches: Vec<PipelineCache>,
}

impl Pipelines2d {
    pub(crate) fn new(
        build: impl Fn(Blend, Option<wgpu::Face>, u32) -> wgpu::RenderPipeline + 'static,
    ) -> Self {
        let build = Rc::new(build);
        let caches = BLENDS_2D
            .iter()
            .flat_map(|blend| [(*blend, None), (*blend, Some(wgpu::Face::Back))])
            .map(|(blend, cull)| {
                let build = Rc::clone(&build);
                PipelineCache::new(move |sample_count| {
                    build(Blend::of_2d(blend), cull, sample_count)
                })
            })
            .collect();
        Self { caches }
    }

    /// The pipeline a node blending as `blend` draws through; `cull` drops
    /// the triangles whose back faces the viewer.
    pub(crate) fn get(
        &self,
        blend: Blend2d,
        cull: bool,
        sample_count: u32,
    ) -> Rc<wgpu::RenderPipeline> {
        let at = BLENDS_2D.iter().position(|b| *b == blend).unwrap_or(0);
        self.caches[at * 2 + usize::from(cull)].get(sample_count)
    }
}

/// Whether a pipeline takes part in the depth buffer: 3D geometry does, and
/// 2D is ordered by the painter's algorithm instead.
#[derive(Clone, Copy)]
pub(crate) enum Depth {
    Tested,
    Ignored,
    /// Drawn over whatever is already there and leaving the depth as it was:
    /// a node whose `depth_test` is off.
    Over,
}

impl Depth {
    fn state(self) -> Option<wgpu::DepthStencilState> {
        match self {
            Self::Tested => Some(wgpu::DepthStencilState {
                format: Context::depth_format(),
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            Self::Ignored => None,
            Self::Over => Some(wgpu::DepthStencilState {
                format: Context::depth_format(),
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
        }
    }
}

/// How a material's triangles land: which faces it culls, whether it takes
/// part in the depth buffer, and how its colour blends.
pub(crate) struct Raster {
    /// `None` for anything a rig can turn inside out, since culling would
    /// drop the triangle it flipped.
    pub(crate) cull: Option<wgpu::Face>,
    pub(crate) depth: Depth,
    pub(crate) blend: Blend,
}

/// One material's pipeline at one sample count.
///
/// `buffers` are the vertex layouts in the order the shader declares them.
pub(crate) fn material_pipeline(
    label: &'static str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
    raster: &Raster,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    Context::get().create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                // The HDR rasterization target; the resolve pass tonemaps.
                format: Context::render_format(),
                blend: raster.blend.state(),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: raster.cull,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: raster.depth.state(),
        multisample: multisample_state(sample_count),
        multiview_mask: None,
        cache: None,
    })
}

/// A 3D material's pipeline for the fork's order-independent transparency
/// pass: its two targets and blends, tested against the opaque depth without
/// writing it, as the fork's own surfaces draw there.
pub(crate) fn transparent_pipeline(
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
    cull: Option<wgpu::Face>,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    use kiss3d::post_processing::{OIT_ACCUM_FORMAT, OIT_REVEAL_FORMAT};
    let add = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    };
    Context::get().create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("material3d_pipeline_transparent"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(crate::material::TRANSPARENT_ENTRY),
            targets: &[
                Some(wgpu::ColorTargetState {
                    format: OIT_ACCUM_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: add,
                        alpha: add,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                // What still shows through: each surface multiplies it by 1 - alpha.
                Some(wgpu::ColorTargetState {
                    format: OIT_REVEAL_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Zero,
                            dst_factor: wgpu::BlendFactor::OneMinusSrc,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::REPLACE,
                    }),
                    write_mask: wgpu::ColorWrites::RED,
                }),
            ],
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
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: multisample_state(sample_count),
        multiview_mask: None,
        cache: None,
    })
}

/// The geometry prepass's pipeline: the four targets the screen-space passes
/// read, single-sampled, writing depth as the colour pass will test it.
///
/// The formats are the fork's own for `renderer::Ssao`'s G-buffer, and the
/// order is the one `shaders/prepass.wesl` writes its locations in.
pub(crate) fn prepass_pipeline(
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
    cull: Option<wgpu::Face>,
) -> wgpu::RenderPipeline {
    let target = Some(wgpu::ColorTargetState {
        format: wgpu::TextureFormat::Rgba16Float,
        blend: None,
        write_mask: wgpu::ColorWrites::ALL,
    });
    Context::get().create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("material3d_pipeline_prepass"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers,
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[target.clone(), target.clone(), target.clone(), target],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            // Whatever the colour pass culls, and nothing else: a face the
            // two disagree about is drawn and never measured.
            cull_mode: cull,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: Depth::Tested.state(),
        multisample: multisample_state(1),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    use super::Blend;
    use kiss3d::scene::Blend2d;
    use kiss3d::wgpu::BlendState;

    #[test]
    fn a_premultiplied_texture_is_blended_without_its_alpha_again() {
        let premultiplied = Blend::of_2d(Blend2d::PremultipliedAlpha);
        assert_eq!(
            premultiplied.state(),
            Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING)
        );
        assert_eq!(
            Blend::of_2d(Blend2d::Alpha).state(),
            Some(BlendState::ALPHA_BLENDING)
        );
    }

    #[test]
    fn every_2d_blend_mode_builds_its_own_state() {
        assert_eq!(
            Blend::of_2d(Blend2d::Additive).state(),
            Blend2d::Additive.blend_state()
        );
        assert_eq!(
            Blend::of_2d(Blend2d::Screen).state(),
            Blend2d::Screen.blend_state()
        );
        assert_eq!(Blend::of_2d(Blend2d::Opaque).state(), None);
    }
}
