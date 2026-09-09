//! The wgpu bind group layout entries every material declares the same way.
//!
//! Each material builds its own layouts — the groups and their labels are
//! that pipeline's business — but the entries inside them are the same shape
//! wherever a shader samples a texture, so they are written once here rather
//! than copied into each.

use kiss3d::context::Context;
use kiss3d::resource::Texture;

use crate::probe::Probe;

/// A uniform buffer binding read by both stages.
pub(crate) fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// The group a shader's own `Params` are bound in, plus the probe bindings
/// after them when the shader reads one.
///
/// `None` when it wants neither. A uniform buffer cannot be zero-sized, so a
/// probing shader with no `Params` still gets a placeholder at binding 0.
/// `prefix` only names the three debug labels, which is all that separates a
/// 2D material's group from a 3D one's.
pub(crate) fn material_group(
    values: &[u8],
    probe: Option<&Probe>,
    prefix: &str,
) -> Option<(wgpu::BindGroupLayout, wgpu::BindGroup)> {
    if values.is_empty() && probe.is_none() {
        return None;
    }
    let ctxt = Context::get();
    let mut layout_entries = vec![uniform_entry(0)];
    if probe.is_some() {
        layout_entries.extend(Probe::layout_entries());
    }
    let layout = ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&format!("{prefix}_params_layout")),
        entries: &layout_entries,
    });
    let placeholder = [0u8; 16];
    let buffer = ctxt.create_buffer_init(
        Some(&format!("{prefix}_params_uniform")),
        if values.is_empty() {
            &placeholder
        } else {
            values
        },
        wgpu::BufferUsages::UNIFORM,
    );
    let mut entries = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: buffer.as_entire_binding(),
    }];
    if let Some(probe) = probe {
        entries.extend(probe.entries());
    }
    let group = ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&format!("{prefix}_params_bind_group")),
        layout: &layout,
        entries: &entries,
    });
    Some((layout, group))
}

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

/// The layout for a group holding `count` textures, each with its sampler, at
/// consecutive pairs of bindings.
pub(crate) fn sampled_slots_layout(
    ctxt: &Context,
    label: &str,
    count: u32,
) -> wgpu::BindGroupLayout {
    let entries: Vec<wgpu::BindGroupLayoutEntry> = (0..count)
        .flat_map(|slot| sampled_entries(slot * 2))
        .collect();
    ctxt.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &entries,
    })
}

/// A group binding one texture and sampler per slot, in slot order: what
/// every shader that imports `package::mesh` reads from group 2.
pub(crate) fn sampled_slots_group(
    ctxt: &Context,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    bound: &[&Texture],
) -> wgpu::BindGroup {
    let entries: Vec<wgpu::BindGroupEntry<'_>> = bound
        .iter()
        .enumerate()
        .flat_map(|(slot, texture)| {
            let first = slot as u32 * 2;
            [
                wgpu::BindGroupEntry {
                    binding: first,
                    resource: wgpu::BindingResource::TextureView(&texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: first + 1,
                    resource: wgpu::BindingResource::Sampler(&texture.sampler),
                },
            ]
        })
        .collect();
    ctxt.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &entries,
    })
}
