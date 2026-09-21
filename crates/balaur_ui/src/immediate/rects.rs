//! `ui.*` bindings: where something was drawn, for a caller placing anything
//! against it — the editor reads its own shell back this way, and a showcase
//! puts its drawn cursor on the control it is about to press.

use balaur_core::Engine;
use balaur_script::{Bindings, BindingsExt, Value};

use crate::vocabulary::keys as k;

/// A drawn box as the script sees it, or nil for one that has not drawn.
fn rect_value(rect: Option<egui::Rect>) -> Value {
    rect.map_or(Value::Nil, |r| {
        Value::Map(vec![
            (k::X.into(), Value::Num(f64::from(r.min.x))),
            (k::Y.into(), Value::Num(f64::from(r.min.y))),
            (k::W.into(), Value::Num(f64::from(r.width()))),
            (k::H.into(), Value::Num(f64::from(r.height()))),
        ])
    })
}

pub(crate) fn install_rects(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        (
            "widget_rect",
            &[],
            "",
            "Where a `widget` node was last drawn, as `#{ x, y, w, h }` in design pixels; empty until it has drawn once.",
        ),
        (
            "tab_rect",
            &[],
            "",
            "Where a `tab` page's own button in the strip was last drawn, as `#{ x, y, w, h }` in design pixels; empty before the strip has drawn. `widget_rect` on the same node answers with the page body.",
        ),
        (
            "scroll_offset",
            &[],
            "",
            "How far a `scroll` node has been scrolled, as `#{ x, y }` in design pixels; zero before it has drawn. What a list building only the rows in view reads to know which ones they are.",
        ),
        (
            "pill_rect",
            &[],
            "",
            "Where the last `pill` was drawn, as `#{ x, y, w, h }` in design pixels; empty before one has. An immediate control has no node `widget_rect` can be asked about, so a caller that wants to point at one reads it back here, straight after the call that drew it.",
        ),
    ]);
    m.function(
        "widget_rect",
        |_eng: &Engine, node: balaur_script::NodeId| {
            Ok(rect_value(crate::widget::arrange::drawn_at(
                balaur_core::entity_of(node)?,
            )))
        },
    );
    m.function("tab_rect", |_eng: &Engine, node: balaur_script::NodeId| {
        Ok(rect_value(crate::widget::arrange::tab_head_at(
            balaur_core::entity_of(node)?,
        )))
    });
    m.function("pill_rect", |_eng: &Engine, (): ()| {
        Ok(rect_value(crate::immediate::last_pill()))
    });
    m.function(
        "scroll_offset",
        |_eng: &Engine, node: balaur_script::NodeId| {
            let entity = balaur_core::entity_of(node)?;
            let offset = crate::widget::scroll::offset_of(entity);
            Ok(Value::Map(vec![
                (k::X.into(), Value::Num(f64::from(offset.x))),
                (k::Y.into(), Value::Num(f64::from(offset.y))),
            ]))
        },
    );
}
