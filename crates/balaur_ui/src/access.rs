//! The drawn widgets as an AccessKit tree, for a screen reader: each widget a
//! node with its role, name, value and states, built at the end of a pass
//! while a reader is listening and handed over where it changed; and what a
//! reader asks of a node applied to its widget.
//!
//! The window layer owns the platform adapter: it calls [`enable_access`]
//! once the platform asks for a tree, [`take_access_update`] after each
//! pass, and [`act`] with each action request.

use balaur_core::Engine;
use balaur_core::hecs::Entity;
use balaur_script::Value;
use egui::accesskit::{
    Action, ActionData, ActionRequest, Node, NodeId, Rect, Role, Toggled, TreeId, TreeInfo,
    TreeUpdate,
};
use rustc_hash::FxHashMap;

use crate::vocabulary::words as w;
use crate::widget::arena::Placed;
use crate::widget::node::{Move, UiFocus, Widget};

/// The node every root widget hangs from: the surface itself. An entity's
/// bits are never zero, so no widget takes this id.
const ROOT: NodeId = NodeId(0);

/// What has been handed to the reader, and what changed since.
#[derive(Default)]
pub struct AccessTree {
    /// Whether a reader asked for the tree; nothing is built until one does.
    enabled: bool,
    /// Each node as last handed over, by id: what a pass compares against.
    sent: FxHashMap<u64, Node>,
    /// The nodes built since the last update was taken, by id.
    changed: FxHashMap<u64, Node>,
    /// Whether the next update carries the tree's own description, which the
    /// first one has to.
    described: bool,
    focus: Option<NodeId>,
}

/// Start building the tree, from the next pass: what the window layer calls
/// once the platform's reader asks for one.
pub fn enable_access(eng: &Engine) {
    if let Some(tree) = eng.try_resource::<AccessTree>() {
        let mut tree = tree.borrow_mut();
        tree.enabled = true;
        tree.sent.clear();
        tree.described = false;
    }
}

/// What changed since the last call, as an update for the platform adapter;
/// `None` while no reader listens or nothing moved.
pub fn take_access_update(eng: &Engine) -> Option<TreeUpdate> {
    let tree = eng.try_resource::<AccessTree>()?;
    let mut tree = tree.borrow_mut();
    if !tree.enabled || (tree.changed.is_empty() && tree.described) {
        return None;
    }
    let tree = &mut *tree;
    let nodes: Vec<(NodeId, Node)> = tree
        .changed
        .drain()
        .map(|(id, node)| (NodeId(id), node))
        .collect();
    let described = std::mem::replace(&mut tree.described, true);
    Some(TreeUpdate {
        nodes,
        tree: (!described).then(|| TreeInfo {
            root: ROOT,
            toolkit_name: Some("balaur".into()),
            toolkit_version: None,
        }),
        tree_id: TreeId::ROOT,
        focus: tree.focus.unwrap_or(ROOT),
    })
}

/// Build the tree from the pass just drawn, and keep the nodes that differ
/// from what the reader already holds.
pub(crate) fn written(
    eng: &Engine,
    ctx: &egui::Context,
    placed: &[Placed],
    roots: &[usize],
    shown: &[usize],
) {
    let Some(tree) = eng.try_resource::<AccessTree>() else {
        return;
    };
    let mut tree = tree.borrow_mut();
    if !tree.enabled {
        return;
    }
    let scale = f64::from(ctx.pixels_per_point());
    let drawn = |index: &usize| shown.contains(index);
    let mut built: FxHashMap<u64, Node> = FxHashMap::default();
    let mut root = Node::new(Role::Window);
    root.set_children(
        roots
            .iter()
            .filter(|index| drawn(index))
            .map(|&index| id_of(placed[index].entity))
            .collect::<Vec<_>>(),
    );
    built.insert(ROOT.0, root);
    for &index in shown {
        let one = &placed[index];
        let mut node = node_of(eng, &one.widget, scale, one.entity);
        node.set_children(
            one.children
                .iter()
                .filter(|child| drawn(child))
                .map(|&child| id_of(placed[child].entity))
                .collect::<Vec<_>>(),
        );
        built.insert(id_of(one.entity).0, node);
    }
    let focused = eng
        .try_resource::<UiFocus>()
        .and_then(|focus| focus.borrow().focused)
        .map(id_of)
        .filter(|id| built.contains_key(&id.0));
    let tree = &mut *tree;
    tree.focus = focused;
    tree.sent.retain(|id, _| built.contains_key(id));
    for (id, node) in built {
        if tree.sent.get(&id) != Some(&node) {
            tree.sent.insert(id, node.clone());
            tree.changed.insert(id, node);
        }
    }
}

/// Apply what a reader asked of a node: focus it, press it, set its value,
/// step it, open or shut it, or bring it into view.
pub fn act(eng: &Engine, request: &ActionRequest) {
    let Some(entity) = Entity::from_bits(request.target_node.0) else {
        return;
    };
    let focus = |taking: bool| {
        if let Some(focus) = eng.try_resource::<UiFocus>() {
            let mut focus = focus.borrow_mut();
            focus.focused = Some(entity);
            focus.taking = taking;
            focus.arrived = true;
        }
    };
    match (request.action, &request.data) {
        (Action::Focus | Action::ScrollIntoView, _) => focus(request.action == Action::Focus),
        (Action::Blur, _) => {
            if let Some(focus) = eng.try_resource::<UiFocus>() {
                focus.borrow_mut().pending = Some(Move::Leave);
            }
        }
        (Action::Click, _) => {
            focus(false);
            if let Some(focus) = eng.try_resource::<UiFocus>() {
                focus.borrow_mut().pending = Some(Move::Accept);
            }
        }
        (Action::SetValue, Some(ActionData::Value(text))) => {
            crate::widget::input::edit(eng, entity, &Value::Str(text.to_string()));
        }
        (Action::SetValue, Some(ActionData::NumericValue(value))) => {
            crate::widget::input::edit(eng, entity, &Value::Num(*value));
        }
        (Action::Increment | Action::Decrement, _) => {
            let stepped = eng.world().get::<&Widget>(entity).ok().map(|widget| {
                let step = if widget.step > 0.0 { widget.step } else { 1.0 };
                let by = if request.action == Action::Increment {
                    step
                } else {
                    -step
                };
                f64::from(widget.value + by)
            });
            if let Some(value) = stepped {
                crate::widget::input::edit(eng, entity, &Value::Num(value));
            }
        }
        (Action::Expand | Action::Collapse, _) => {
            let open = request.action == Action::Expand;
            crate::widget::input::edit(eng, entity, &Value::Bool(open));
        }
        _ => {}
    }
}

/// The node an entity's widget is, by the entity's bits.
fn id_of(entity: Entity) -> NodeId {
    NodeId(entity.to_bits().get())
}

/// What a reader says a kind is.
fn role_of(widget: &Widget) -> Role {
    match widget.kind.as_str() {
        w::LABEL => Role::Label,
        w::BUTTON if widget.toggle => Role::Switch,
        w::BUTTON => Role::Button,
        w::CHECKBOX => Role::CheckBox,
        w::SWITCH => Role::Switch,
        w::TEXT_FIELD => Role::TextInput,
        w::TEXT_AREA => Role::MultilineTextInput,
        w::CODE => Role::Code,
        w::SLIDER => Role::Slider,
        w::NUMBER_FIELD => Role::SpinButton,
        w::PROGRESS_BAR => Role::ProgressIndicator,
        w::DROPDOWN => Role::ComboBox,
        w::MENU => Role::Menu,
        w::LIST => Role::ListBox,
        w::TREE => Role::Tree,
        w::TABLE => Role::Table,
        w::IMAGE => Role::Image,
        w::COLOR_PICKER => Role::ColorWell,
        w::SCROLL => Role::ScrollView,
        w::TABS => Role::TabList,
        w::DIALOG => Role::Dialog,
        w::WINDOW => Role::Window,
        w::TOAST => Role::Status,
        w::SEPARATOR => Role::Splitter,
        w::FOLD | w::INSPECTOR => Role::Group,
        _ => Role::GenericContainer,
    }
}

/// One widget as a node: its role and bounds, what it is called, what it
/// holds, its states and what a reader may do to it.
fn node_of(eng: &Engine, widget: &Widget, scale: f64, entity: Entity) -> Node {
    let mut node = Node::new(role_of(widget));
    if let Some(rect) = crate::widget::arrange::drawn_at(entity) {
        node.set_bounds(Rect {
            x0: f64::from(rect.min.x) * scale,
            y0: f64::from(rect.min.y) * scale,
            x1: f64::from(rect.max.x) * scale,
            y1: f64::from(rect.max.y) * scale,
        });
    }
    let caption = crate::widget::layer::caption(eng, widget);
    let kind = widget.kind.as_str();
    match kind {
        // A field's text is what it holds; what it is called is elsewhere.
        w::TEXT_FIELD | w::TEXT_AREA | w::CODE => {
            if !widget.secret {
                node.set_value(caption.as_str());
            }
            if !widget.placeholder.is_empty() {
                node.set_placeholder(widget.placeholder.as_str());
            }
            node.add_action(Action::SetValue);
        }
        w::IMAGE if !widget.egui.alt_text.is_empty() => {
            node.set_label(widget.egui.alt_text.as_str());
        }
        w::DROPDOWN => {
            node.set_value(caption.as_str());
            node.add_action(Action::SetValue);
        }
        _ if !caption.is_empty() => node.set_label(caption.as_str()),
        _ => {}
    }
    if !widget.tooltip.is_empty() {
        node.set_description(widget.tooltip.as_str());
    }
    if matches!(kind, w::SLIDER | w::NUMBER_FIELD | w::PROGRESS_BAR) {
        node.set_numeric_value(f64::from(widget.value));
        node.set_min_numeric_value(f64::from(widget.min));
        node.set_max_numeric_value(f64::from(widget.max));
        if widget.step > 0.0 {
            node.set_numeric_value_step(f64::from(widget.step));
        }
        if kind != w::PROGRESS_BAR {
            node.add_action(Action::SetValue);
            node.add_action(Action::Increment);
            node.add_action(Action::Decrement);
        }
    }
    if matches!(kind, w::CHECKBOX | w::SWITCH) || (kind == w::BUTTON && widget.toggle) {
        node.set_toggled(Toggled::from(widget.checked));
    }
    if kind == w::FOLD {
        node.set_expanded(widget.open);
        node.add_action(if widget.open {
            Action::Collapse
        } else {
            Action::Expand
        });
    }
    if widget.disabled {
        node.set_disabled();
    }
    if crate::widget::focus::takes_focus(widget) {
        node.add_action(Action::Focus);
        node.add_action(Action::Click);
    }
    node
}
