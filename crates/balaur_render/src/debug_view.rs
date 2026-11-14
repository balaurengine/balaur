//! The views a shader author asks the viewport for: one channel of the scene
//! instead of the picture, or the value a shader line computes.
//!
//! Both are read by the backends when they build a node's material, and
//! neither can reach the simulation: rendering is an observer whichever of
//! them is on.

use anyhow::anyhow;
use balaur_core::Engine;
use balaur_script::Module;

use crate::shaders;

const DEBUG_VIEW: &str = "debug_view";
const DEBUG_VIEWS: &str = "debug_views";
const SET_DEBUG_VIEW: &str = "set_debug_view";
const SET_SHADER_PREVIEW: &str = "set_shader_preview";

/// Which channel a windowed backend draws instead of the scene's colour:
/// one of [`shaders::CHANNELS`], or empty for the scene as it is.
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
#[cfg(feature = "window")]
pub(crate) fn probe_at(eng: &Engine) -> Option<[f32; 2]> {
    eng.try_resource::<ProbeRequest>()
        .map(|request| request.borrow().at)
}

/// Record what the shader wrote for the probed pixel.
///
/// `None` clears it: a pixel that drew nothing has no value, and leaving the
/// last one standing would report a number for empty space.
#[cfg(feature = "window")]
pub(crate) fn publish_probe(eng: &Engine, value: Option<[f32; 4]>) {
    match value {
        Some(value) => eng.insert_resource(ProbeReading { value }),
        None => eng.remove_resource::<ProbeReading>(),
    }
}

/// Which channel a backend is drawing, or empty for the scene as it is.
#[cfg(feature = "window")]
pub(crate) fn channel_view(eng: &Engine) -> String {
    eng.try_resource::<ChannelView>()
        .map_or_else(String::new, |view| view.borrow().channel.clone())
}

/// `render::set_debug_view`, `render::debug_view`, `render::debug_views` and
/// `render::set_shader_preview`.
pub(crate) fn install_debug_view_api(m: &mut Module<Engine>) {
    m.describe(&[
        (SET_DEBUG_VIEW, &[], "", "Draw one channel of the scene (normals, uv, depth or albedo) instead of its colour; an empty name puts the picture back."),
        (DEBUG_VIEW, &[], "", "Which channel the viewport is drawing instead of the scene's colour, or empty for the scene as it is."),
        (DEBUG_VIEWS, &[], "-> [string]", "Every channel name `set_debug_view` accepts, as a list."),
        (SET_SHADER_PREVIEW, &[], "", "Draw the value a shader's line computes for every pixel that reaches it; line 0 puts the picture back."),
    ]);
    // Draw one channel of the scene instead of its colour: "normals", "uv",
    // "depth", "albedo", or "" for the scene as it is.
    m.function(SET_DEBUG_VIEW, |eng: &Engine, channel: String| {
        if !channel.is_empty() && !shaders::CHANNELS.contains(&channel.as_str()) {
            return Err(anyhow!(
                "no channel '{channel}'; the channels are {}",
                shaders::CHANNELS.join(", ")
            ));
        }
        eng.insert_resource(ChannelView { channel });
        Ok(())
    });
    m.function(DEBUG_VIEW, |eng: &Engine, ()| {
        Ok(eng
            .try_resource::<ChannelView>()
            .map_or_else(String::new, |view| view.borrow().channel.clone()))
    });
    m.function(DEBUG_VIEWS, |_eng: &Engine, ()| {
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
        SET_SHADER_PREVIEW,
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
}
