//! The pixel a shader preview is asked about, and the value read back from it.
//!
//! `preview` rewrites a shader to write what it computed for exactly one
//! pixel into a storage buffer; this owns that buffer, the uniform naming the
//! pixel, and the copy back to the host. Sampling the drawn frame instead
//! would give the tonemapped 8-bit colour rather than the shader's own float,
//! which is a plausible number and the wrong one.
//!
//! Bindings 1 and 2 of the material's own bind group, not a group of their
//! own: WebGPU guarantees four groups and the material already spends them.

use kiss3d::context::Context;

/// Four floats: what one invocation computed.
const SIZE: u64 = 16;

/// The buffers a previewing material binds beside its `Params`.
pub(crate) struct Probe {
    /// What the shader writes.
    out: wgpu::Buffer,
    /// The pixel it is asked about, as `balaur_probe_at`.
    at: wgpu::Buffer,
    /// Host-visible copy of `out`; a storage buffer cannot be mapped.
    staging: wgpu::Buffer,
}

impl Probe {
    pub(crate) fn new() -> Self {
        let ctxt = Context::get();
        Self {
            out: ctxt.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shader_probe"),
                size: SIZE,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            at: ctxt.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shader_probe_at"),
                size: SIZE,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            staging: ctxt.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shader_probe_staging"),
                size: SIZE,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        }
    }

    /// The layout entries the previewing shader declares.
    pub(crate) fn layout_entries() -> [wgpu::BindGroupLayoutEntry; 2] {
        [
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ]
    }

    pub(crate) fn entries(&self) -> [wgpu::BindGroupEntry<'_>; 2] {
        [
            wgpu::BindGroupEntry {
                binding: 1,
                resource: self.out.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: self.at.as_entire_binding(),
            },
        ]
    }

    /// Point the probe at a pixel, in framebuffer coordinates.
    ///
    /// Clears what was written last: a pixel that draws nothing writes
    /// nothing, and the answer has to be "nothing drew there" rather than
    /// whatever the last pixel said.
    pub(crate) fn aim(&self, at: [f32; 2]) {
        let ctxt = Context::get();
        let uniform = [at[0], at[1], 0.0, 0.0];
        ctxt.write_buffer(&self.at, 0, bytemuck::bytes_of(&uniform));
        ctxt.write_buffer(&self.out, 0, &[0u8; SIZE as usize]);
    }

    /// What the shader wrote, or `None` when nothing drew at that pixel.
    ///
    /// Every encoding `preview` emits sets the fourth channel to 1, so a zero
    /// there is the cleared buffer and not a value.
    ///
    /// Copies, maps and waits, so this is a debugging call and not something
    /// a frame does: it stalls until the GPU catches up.
    pub(crate) fn read(&self) -> Option<[f32; 4]> {
        let ctxt = Context::get();
        let mut encoder = ctxt.create_command_encoder(Some("shader_probe_readback"));
        encoder.copy_buffer_to_buffer(&self.out, 0, &self.staging, 0, SIZE);
        ctxt.submit(std::iter::once(encoder.finish()));

        let slice = self.staging.slice(..);
        // wgpu answers the buffer map on this channel and nothing on the far
        // end leaves the process; rendering is an observer either way.
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        ctxt.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .ok()?;
        receiver.recv().ok()?.ok()?;

        let mapped = slice.get_mapped_range().ok()?;
        let bytes: [u8; SIZE as usize] = mapped[..SIZE as usize].try_into().ok()?;
        drop(mapped);
        self.staging.unmap();
        if bytes[12..16] == [0; 4] {
            return None;
        }
        Some([
            f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            f32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            f32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        ])
    }
}
