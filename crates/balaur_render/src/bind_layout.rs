//! The wgpu bind group layout entries every material declares the same way.
//!
//! Each material builds its own layouts — the groups and their labels are
//! that pipeline's business — but the entries inside them are the same shape
//! wherever a shader samples a texture, so they are written once here rather
//! than copied into each.

use kiss3d::context::Context;

/// A texture and its sampler at two consecutive bindings, both read by the
/// fragment stage.
pub(crate) fn sampled_entries(first: u32) -> [wgpu::BindGroupLayoutEntry; 2] {
    [
        wgpu::BindGroupLayoutEntry {
            binding: first,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: first + 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
    ]
}

/// The layout for a group holding one texture and its sampler.
pub(crate) fn sampled_layout(ctxt: &Context, label: &str) -> wgpu::BindGroupLayout {
    ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &sampled_entries(0),
    })
}
