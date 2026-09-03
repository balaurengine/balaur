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

/// Which channel a backend is drawing, or empty for the scene as it is.
#[cfg(feature = "kiss3d")]
pub(crate) fn channel_view(eng: &Engine) -> String {
    eng.try_resource::<ChannelView>()
        .map_or_else(String::new, |view| view.borrow().channel.clone())
}

/// `render::set_channel`, `render::channel`, `render::channels` and
/// `render::set_shader_preview`.
pub(crate) fn install_debug_view_api(m: &mut dyn Bindings<Engine>) {
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
}
