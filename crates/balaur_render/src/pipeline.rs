//! The render pipeline every Balaur material builds.
//!
//! All four — 2D and 3D materials, skinned polygons and skinned meshes —
//! rasterize triangles into the same alpha-blended target with the same
//! entry point names; they differ only in their vertex buffers, whether they
//! cull, and whether they test depth. Those are the arguments here, and the
//! rest of the descriptor is written once.

use kiss3d::context::Context;
use kiss3d::resource::multisample_state;
use kiss3d::wgpu;

/// Whether a pipeline takes part in the depth buffer: 3D geometry does, and
/// 2D is ordered by the painter's algorithm instead.
pub(crate) enum Depth {
    Tested,
    Ignored,
}

impl Depth {
    fn state(&self) -> Option<wgpu::DepthStencilState> {
        match self {
            Self::Tested => Some(wgpu::DepthStencilState {
                format: Context::depth_format(),
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            Self::Ignored => None,
        }
    }
}

/// One material's pipeline at one sample count.
///
/// `buffers` are the vertex layouts in the order the shader declares them;
/// `cull` is `None` for anything a rig can turn inside out, since culling
/// would drop the triangle it flipped.
pub(crate) fn material_pipeline(
    label: &'static str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
    cull: Option<wgpu::Face>,
    depth: &Depth,
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
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
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
        depth_stencil: depth.state(),
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
