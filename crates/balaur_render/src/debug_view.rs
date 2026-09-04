//! The views a shader author asks the viewport for: one channel of the scene
//! instead of the picture, or the value a shader line computes.
//!
//! Both are read by the backends when they build a node's material, and
//! neither can reach the simulation — rendering is an observer whichever of
//! them is on.

use anyhow::anyhow;
use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt};

use crate::shaders;

/// Which channel a windowed backend draws instead of the scene's colour —
/// one of [`shaders::CHANNELS`] — or empty for the scene as it is.
///
/// A debugging view. Rendering stays an observer either way, so what is on
/// screen never reaches the simulation.
#[derive(Default)]
pub struct ChannelView {
    pub channel: String,
}

/// The shader line a backend draws the value of, instead of the picture.
///
/// Set by the editor as a caret moves; every material naming that shader is
/// relinked through `preview::preview`.
#[derive(Default)]
pub struct PreviewRequest {
    /// Project-relative path of the shader, or empty for no preview.
    pub shader: String,
    /// The 1-based line whose value is drawn.
    pub line: usize,
}

/// The pixel a preview is asked the value of, in framebuffer coordinates.
#[derive(Default)]
pub struct ProbeRequest {
    pub at: [f32; 2],
}

/// What the shader wrote for that pixel, one frame ago.
#[derive(Default)]
pub struct ProbeReading {
    pub value: [f32; 4],
}

/// The pixel a preview is asked about, or `None` when nobody asked.
#[cfg(feature = "kiss3d")]
pub(crate) fn probe_at(eng: &Engine) -> Option<[f32; 2]> {
    eng.try_resource::<ProbeRequest>()
        .map(|request| request.borrow().at)
}

/// Record what the shader wrote, for `render::shader_probe` to answer with.
///
/// `None` clears it: a pixel that drew nothing has no value, and leaving the
/// last one standing would report a number for empty space.
#[cfg(feature = "kiss3d")]
pub(crate) fn publish_probe(eng: &Engine, value: Option<[f32; 4]>) {
    match value {
        Some(value) => eng.insert_resource(ProbeReading { value }),
        None => eng.remove_resource::<ProbeReading>(),
    }
}

/// Which channel a backend is drawing, or empty for the scene as it is.
#[cfg(feature = "kiss3d")]
pub(crate) fn channel_view(eng: &Engine) -> String {
    eng.try_resource::<ChannelView>()
        .map_or_else(String::new, |view| view.borrow().channel.clone())
}

/// `render::set_channel`, `render::channel`, `render::channels` and
/// `render::set_shader_preview`.
pub(crate) fn install_debug_view_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_channel", &[], "", "Draw one channel of the scene — normals, uv, depth or albedo — instead of its colour; an empty name puts the picture back."),
        ("channel", &[], "", "Which channel the viewport is drawing instead of the scene's colour, or empty for the scene as it is."),
        ("channels", &[], "", "Every channel name `set_channel` accepts, as a list."),
        ("set_shader_preview", &[], "", "Draw the value a shader's line computes for every pixel that reaches it; line 0 puts the picture back."),
        ("set_shader_probe", &[], "", "Ask what the previewed line computed at one framebuffer pixel; the answer arrives through `shader_probe` a frame later."),
        ("shader_probe", &[], "", "The four channels the previewed line wrote at the probed pixel, or `()` when nothing has been read yet."),
    ]);
    // Draw one channel of the scene instead of its colour: "normals", "uv",
    // "depth", "albedo", or "" for the scene as it is.
    m.function("set_channel", |eng: &Engine, channel: String| {
        if !channel.is_empty() && !shaders::CHANNELS.contains(&channel.as_str()) {
            return Err(anyhow!(
                "no channel '{channel}'; the channels are {}",
                shaders::CHANNELS.join(", ")
            ));
        }
        eng.insert_resource(ChannelView { channel });
        Ok(())
    });
    m.function("channel", |eng: &Engine, ()| {
        Ok(eng
            .try_resource::<ChannelView>()
            .map_or_else(String::new, |view| view.borrow().channel.clone()))
    });
    m.function("channels", |_eng: &Engine, ()| {
        Ok(balaur_script::Value::List(
            shaders::CHANNELS
                .iter()
                .map(|c| balaur_script::Value::Str((*c).to_string()))
                .collect(),
        ))
    });
    // Draw the value the shader's `line` computes, for every pixel that
    // reaches it. A line of 0 puts the picture back.
    m.function(
        "set_shader_preview",
        |eng: &Engine, (shader, line): (String, i64)| {
            let line = usize::try_from(line.max(0)).unwrap_or(0);
            eng.insert_resource(PreviewRequest {
                shader: if line == 0 { String::new() } else { shader },
                line,
            });
            // The materials naming it have to be linked again.
            balaur_core::assets::invalidate(eng);
            Ok(())
        },
    );
    // Ask what a previewed line computed at one pixel. The answer arrives
    // through `shader_probe` a frame later: reading it waits on the GPU, and
    // a frame does not.
    m.function("set_shader_probe", |eng: &Engine, (x, y): (f64, f64)| {
        eng.insert_resource(ProbeRequest {
            at: [x as f32, y as f32],
        });
        Ok(())
    });
    m.function("shader_probe", |eng: &Engine, ()| {
        Ok(eng
            .try_resource::<ProbeReading>()
            .map_or(balaur_script::Value::Nil, |reading| {
                let value = reading.borrow().value;
                balaur_script::Value::List(
                    value
                        .iter()
                        .map(|v| balaur_script::Value::Num(f64::from(*v)))
                        .collect(),
                )
            }))
    });
}
