//! A `.tscn` as a balaur scene.
//!
//! The walk is Godot's own order, so a parent is always written before its
//! children. What differs is how an instance is held: a Godot instance node
//! *is* the prefab's root, renamed, while here an `instance` node holds the
//! prefab's roots as its children. So what Godot writes on an instance line,
//! and on any node edited inside one, lands in `overrides` under the path
//! from the prefab's root.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use balaur_plugin::toml;
use toml::Value as Toml;

use crate::godot::{Document, Section, Value};
use crate::godot::anim::{join, node_path};
use crate::godot::nodes::{Family, Mapped, Resources, family, map};

/// A converted scene, the files written beside it, and what did not carry.
pub(crate) struct Converted {
    pub scene_toml: String,
    /// Clip libraries, by project path.
    pub files: Vec<(String, String)>,
    pub notes: Vec<String>,
}

/// What an instance of a scene needs to know about it: every node's class and
/// script by its path under the root.
struct Outline {
    classes: BTreeMap<String, String>,
    /// The Godot script each scripted node carries, project-relative.
    scripts: BTreeMap<String, String>,
    /// The nodes that are instances themselves, to the scene each holds.
    instances: BTreeMap<String, String>,
}

/// A scene's path in the converted project: the same tree, `.toml`.
pub(crate) fn scene_path(godot: &str) -> String {
    match godot.strip_suffix(".tscn") {
        Some(stem) => format!("{stem}.toml"),
        None => godot.to_string(),
    }
}

/// A script's path in the converted project: the same tree, `.rn`.
fn script_path(godot: &str) -> String {
    match godot.strip_suffix(".gd") {
        Some(stem) => format!("{stem}.rn"),
        None => godot.to_string(),
    }
}

/// Where a node's keys are written: its own table, or an override table on
/// the instance above it.
#[derive(Clone)]
enum Slot {
    Own(usize),
    Override { instance: usize, path: String },
}

/// What a widget emits by name when its value changes and when a field is
/// submitted; `balaur_ui`'s `CHANGE_EVENT` and `SUBMIT_EVENT`.
const CHANGE_EVENT: &str = "change";
const SUBMIT_EVENT: &str = "submit";

/// Godot signals of its own classes that nothing here emits; a row answering
/// one waits on a script that does.
const UNSENT: &[&str] = &[
    "gui_input",
    "visibility_changed",
    "text_change_rejected",
    "tab_changed",
    "tab_selected",
    "draw",
    "resized",
    "ready",
    "tree_entered",
    "tree_exited",
];

struct Walk<'a> {
    res: Resources<'a>,
    nodes: Vec<toml::Table>,
    assets: Vec<Toml>,
    asset_ids: BTreeMap<String, String>,
    slots: BTreeMap<String, Slot>,
    classes: BTreeMap<String, String>,
    instances: BTreeMap<String, Outline>,
    ids: BTreeMap<String, String>,
    taken: Vec<String>,
    notes: Vec<String>,
    /// Files nodes asked to have written beside the scene.
    files: Vec<(String, String)>,
}

/// Convert one scene. `path` is its project path, for naming the files it
/// writes beside itself; `root` is the Godot project's root.
pub(crate) fn convert(
    document: &Document,
    path: &str,
    root: &Path,
    project: &crate::godot::nodes::Project,
) -> Result<Converted> {
    let mut walk = Walk {
        res: crate::godot::nodes::resources_of(document, root, project),
        nodes: Vec::new(),
        assets: Vec::new(),
        asset_ids: BTreeMap::new(),
        slots: BTreeMap::new(),
        classes: BTreeMap::new(),
        instances: BTreeMap::new(),
        ids: BTreeMap::new(),
        taken: Vec::new(),
        notes: Vec::new(),
        files: Vec::new(),
    };
    let nodes: Vec<&Section> = document.each("node").collect();
    for section in &nodes {
        walk.node(section);
    }
    for connection in document.each("connection") {
        walk.connection(connection);
    }
    let mut files = std::mem::take(&mut walk.files);
    let stem = path.strip_suffix(".tscn").unwrap_or(path);
    for section in &nodes {
        match section.attr_str("type") {
            Some("AnimationPlayer") => walk.player(section, stem, &mut files),
            Some("AnimationTree") => {
                walk.player(section, stem, &mut files);
                walk.machine(section, stem, &mut files);
            }
            _ => {}
        }
    }
    let mut out = toml::Table::new();
    if !walk.assets.is_empty() {
        out.insert("assets".into(), Toml::Array(walk.assets));
    }
    out.insert(
        "nodes".into(),
        Toml::Array(walk.nodes.into_iter().map(Toml::Table).collect()),
    );
    let mut scene_toml = format!("# Converted from {path} by `balaur import`.\n\n");
    scene_toml.push_str(&toml::to_string(&Toml::Table(out))?);
    Ok(Converted {
        scene_toml,
        files,
        notes: walk.notes,
    })
}

impl Walk<'_> {
    fn node(&mut self, section: &Section) {
        let name = section.attr_str("name").unwrap_or("Node").to_string();
        let parent = section
            .attr_str("parent")
            .map(|p| if p == "." { "" } else { p });
        let path = match parent {
            None => String::new(),
            Some("") => name.clone(),
            Some(p) => format!("{p}/{name}"),
        };
        let parent_class = parent
            .and_then(|p| self.classes.get(p))
            .cloned()
            .unwrap_or_default();

        if let Some(instance) = section.attr("instance") {
            self.instance(section, &name, &path, parent, instance, &parent_class);
            return;
        }
        let Some(class) = section.attr_str("type").map(str::to_string) else {
            self.edit(section, &path, &parent_class);
            return;
        };
        let mapped = map(&class, section, &parent_class, &self.res);
        let id = self.id_for(&path, &name);
        let mut table = toml::Table::new();
        table.insert("id".into(), Toml::String(id.clone()));
        table.insert("name".into(), Toml::String(name));
        table.insert("parent".into(), Toml::String(self.parent_ref(parent)));
        self.nodes.push(table);
        let index = self.nodes.len() - 1;
        self.slots.insert(path.clone(), Slot::Own(index));
        self.classes.insert(path.clone(), class.clone());
        self.write(&path, &class, mapped, &id);
        self.groups(section, index);
        self.script(section, &path);
    }

    /// A node that instances another scene: a node of its own holding the
    /// prefab, and whatever the Godot line set, as overrides on its root.
    fn instance(
        &mut self,
        section: &Section,
        name: &str,
        path: &str,
        parent: Option<&str>,
        instance: &Value,
        parent_class: &str,
    ) {
        let Some(prefab) = self.res.path(instance).map(str::to_string) else {
            self.notes.push(format!(
                "`{path}` instances a scene this file does not declare"
            ));
            return;
        };
        let outline = outline(&self.res, &prefab);
        let id = self.id_for(path, name);
        let mut table = toml::Table::new();
        table.insert("id".into(), Toml::String(id));
        table.insert("name".into(), Toml::String(name.to_string()));
        table.insert("parent".into(), Toml::String(self.parent_ref(parent)));
        table.insert("instance".into(), Toml::String(scene_path(&prefab)));
        self.nodes.push(table);
        let index = self.nodes.len() - 1;
        self.groups(section, index);
        let Some(outline) = outline else {
            self.notes.push(format!(
                "`{path}` instances {prefab}, which would not load; its edits were dropped"
            ));
            self.slots.insert(path.to_string(), Slot::Own(index));
            return;
        };
        let class = outline.classes.get("").cloned().unwrap_or_default();
        let root = Slot::Override {
            instance: index,
            path: ".".into(),
        };
        self.slots.insert(path.to_string(), root);
        self.classes.insert(path.to_string(), class.clone());
        let mapped = map(&class, section, parent_class, &self.res);
        let id = format!("{}_root", self.ids[path]);
        self.write(path, &class, mapped, &id);
        self.script(section, path);
        self.retune(section, path, script_in(&self.res, &outline, ""));
        self.instances.insert(path.to_string(), outline);
    }

    /// A node with no type: an edit of one an instance above it already has.
    fn edit(&mut self, section: &Section, path: &str, parent_class: &str) {
        // The scene's own root may be an instance, whose path is empty and
        // who owns everything below it that no deeper instance does.
        let owner = self
            .instances
            .keys()
            .filter(|inst| inst.is_empty() || path.starts_with(&format!("{inst}/")))
            .max_by_key(|inst| inst.len())
            .cloned();
        let Some(owner) = owner else {
            self.notes.push(format!(
                "`{path}` has no type and sits in no instance; skipped"
            ));
            return;
        };
        let inner = if owner.is_empty() {
            path
        } else {
            &path[owner.len() + 1..]
        };
        let class = class_in(&self.res, &self.instances[&owner], inner, 0);
        let Slot::Override { instance, .. } = self.slots[&owner].clone() else {
            return;
        };
        // Every instance is its prefab's root here, so the path inside one is
        // the Godot path from it.
        self.slots.insert(
            path.to_string(),
            Slot::Override {
                instance,
                path: inner.to_string(),
            },
        );
        self.classes.insert(path.to_string(), class.clone());
        let mapped = map(&class, section, parent_class, &self.res);
        let id = format!("{}_{}", self.ids[&owner], slug(inner));
        self.write(path, &class, mapped, &id);
        self.script(section, path);
        let godot = script_in(&self.res, &self.instances[&owner], inner);
        self.retune(section, path, godot);
    }

    /// The table a node's keys go in, made on first use for an override.
    fn table(&mut self, path: &str) -> Option<&mut toml::Table> {
        match self.slots.get(path)?.clone() {
            Slot::Own(index) => self.nodes.get_mut(index),
            Slot::Override { instance, path } => {
                let node = self.nodes.get_mut(instance)?;
                let overrides = node
                    .entry("overrides")
                    .or_insert_with(|| Toml::Table(toml::Table::new()));
                let Toml::Table(overrides) = overrides else {
                    return None;
                };
                match overrides
                    .entry(path)
                    .or_insert_with(|| Toml::Table(toml::Table::new()))
                {
                    Toml::Table(table) => Some(table),
                    _ => None,
                }
            }
        }
    }

    /// One mapped node into its table, its inline assets into the scene's
    /// `[[assets]]`, its extra nodes under it, and its notes prefixed with
    /// where they came from.
    fn write(&mut self, path: &str, class: &str, mapped: Mapped, id: &str) {
        let Mapped {
            keys,
            mut components,
            assets,
            children,
            notes,
            files,
        } = mapped;
        for file in files {
            if !self.files.iter().any(|(path, _)| *path == file.0) {
                self.files.push(file);
            }
        }
        for (index, asset) in assets.into_iter().enumerate() {
            let reference = self.asset(asset.table, id, asset.component, index);
            if let Some(Toml::Table(table)) = components.get_mut(asset.component) {
                table.insert(asset.key.into(), Toml::String(format!("#{reference}")));
            }
        }
        let shown = if path.is_empty() { "the root" } else { path };
        for note in notes {
            self.notes.push(format!("`{shown}` ({class}): {note}"));
        }
        let Some(table) = self.table(path) else {
            return;
        };
        for (key, value) in keys.into_iter().chain(components) {
            match (table.get_mut(&key), value) {
                (Some(Toml::Table(have)), Toml::Table(more)) => have.extend(more),
                (_, value) => {
                    table.insert(key, value);
                }
            }
        }
        if children.is_empty() {
            return;
        }
        let Some(Slot::Own(_)) = self.slots.get(path) else {
            self.notes.push(format!(
                "`{shown}` ({class}): inside an instance, so its extra nodes were not added"
            ));
            return;
        };
        for (name, child) in children {
            let child_path = if path.is_empty() {
                name.clone()
            } else {
                format!("{path}/{name}")
            };
            let child_id = self.id_for(&child_path, &name);
            let mut table = toml::Table::new();
            table.insert("id".into(), Toml::String(child_id.clone()));
            table.insert("name".into(), Toml::String(name));
            table.insert("parent".into(), Toml::String(id.to_string()));
            self.nodes.push(table);
            self.slots
                .insert(child_path.clone(), Slot::Own(self.nodes.len() - 1));
            self.write(&child_path, class, child, &child_id);
        }
    }

    fn groups(&mut self, section: &Section, index: usize) {
        let Some(groups) = section.attr("groups").and_then(Value::as_array) else {
            return;
        };
        let tags: Vec<Toml> = groups
            .iter()
            .filter_map(Value::as_str)
            .map(|g| Toml::String(g.to_string()))
            .collect();
        if !tags.is_empty() {
            self.nodes[index].insert("tags".into(), Toml::Array(tags));
        }
    }

    /// The node's script, renamed to the `.rn` the script phase writes, and
    /// the values its `@export`s were given here.
    fn script(&mut self, section: &Section, path: &str) {
        let Some(reference) = section.field("script") else {
            return;
        };
        let Some(godot) = self.res.path(reference).map(str::to_string) else {
            self.notes
                .push(format!("`{path}`: an inline script is not converted"));
            return;
        };
        let props = self.script_props(section, path, &godot);
        let mut script = toml::Table::new();
        script.insert("source".into(), Toml::String(script_path(&godot)));
        if !props.is_empty() {
            script.insert("props".into(), Toml::Table(props));
        }
        if let Some(table) = self.table(path) {
            table.insert("script".into(), Toml::Table(script));
        }
    }

    /// The exports a line sets on a node inside an instance whose prefab gave
    /// it the script: a props-only script override, which retunes it.
    fn retune(&mut self, section: &Section, path: &str, godot: Option<String>) {
        let Some(godot) = godot.filter(|_| section.field("script").is_none()) else {
            return;
        };
        let props = self.script_props(section, path, &godot);
        if props.is_empty() {
            return;
        }
        let mut script = toml::Table::new();
        script.insert("props".into(), Toml::Table(props));
        if let Some(table) = self.table(path) {
            table.insert("script".into(), Toml::Table(script));
        }
    }

    /// The values `section` gives the exports of the Godot script `godot`.
    fn script_props(&mut self, section: &Section, path: &str, godot: &str) -> toml::Table {
        let source = std::fs::read_to_string(self.res.root.join(godot)).unwrap_or_default();
        let exports = crate::godot::exports::exports(&source, &self.res.project.classes);
        let mut props = toml::Table::new();
        let res = &self.res;
        let path_of = |v: &Value| res.path(v).map(scene_path);
        for (key, value) in &section.fields {
            let Some(export) = exports.iter().find(|e| &e.name == key) else {
                continue;
            };
            let Some(kind) = export.kind else {
                self.notes.push(format!(
                    "`{path}`: export `{key}` is a {}, which a scene prop cannot hold; dropped",
                    export.hint
                ));
                continue;
            };
            match crate::godot::exports::scene_value(kind, value, &path_of) {
                Some(value) => {
                    props.insert(key.clone(), value);
                }
                None => self.notes.push(format!(
                    "`{path}`: export `{key}`'s value did not read as a {}",
                    export.hint
                )),
            }
        }
        props
    }

    /// A signal connection as the handler key a widget names, or a binding
    /// row on the emitting node: a click, a collision, a pointer crossing,
    /// or `emitted:<signal>` for everything a node emits by name.
    fn connection(&mut self, section: &Section) {
        let (Some(signal), Some(from), Some(to), Some(method)) = (
            section.attr_str("signal"),
            section.attr_str("from"),
            section.attr_str("to"),
            section.attr_str("method"),
        ) else {
            return;
        };
        let from = join("", from);
        let to = join("", to);
        let class = self.classes.get(&from).cloned().unwrap_or_default();
        if section.attr("binds").is_some() || section.attr("unbinds").is_some() {
            self.notes.push(format!(
                "connection `{signal}` from `{from}` to `{method}`: its bound arguments were dropped"
            ));
        }
        // A dialog's answer is a click on the button it was given here.
        let (from, signal) = match (class.as_str(), signal) {
            ("AcceptDialog" | "ConfirmationDialog", "confirmed") => (
                format!("{from}/{}", crate::godot::controls::DIALOG_OK),
                "pressed",
            ),
            ("ConfirmationDialog", "canceled") => (
                format!("{from}/{}", crate::godot::controls::DIALOG_CANCEL),
                "pressed",
            ),
            _ => (from, signal),
        };
        let control = family(&class) == Family::Control;
        // A widget handler runs on the widget's node or the nearest scripted
        // ancestor, so it can say a connection to either and nothing else.
        let upward = to.is_empty() || from == to || from.starts_with(&format!("{to}/"));
        let handler = match signal {
            "pressed" | "button_up" => Some("on_click"),
            "toggled" | "text_changed" | "value_changed" | "item_selected" | "folding_changed"
            | "close_requested" => Some("on_change"),
            "text_submitted" => Some("on_submit"),
            "focus_entered" => Some("on_focus"),
            _ => None,
        };
        if control
            && upward
            && let Some(handler) = handler
        {
            if let Some(Toml::Table(widget)) = self.table(&from).map(|t| {
                t.entry("widget")
                    .or_insert_with(|| Toml::Table(toml::Table::new()))
            }) {
                widget.insert(handler.into(), Toml::String(method.to_string()));
            }
            return;
        }
        if handler.is_none() && UNSENT.contains(&signal) {
            self.notes.push(format!(
                "connection `{signal}` from `{from}`: nothing here emits it, so its row waits on a script's `emit(\"{signal}\")`"
            ));
        }
        let event = event_of(signal, control, handler);
        // A collision is reported by the collider's own node, and a Godot
        // area or body holds its shapes as children, so the row goes on each.
        let colliding = event.starts_with("collision");
        let holders: Vec<String> = if colliding {
            let prefix = if from.is_empty() {
                String::new()
            } else {
                format!("{from}/")
            };
            self.classes
                .iter()
                .filter(|(path, class)| {
                    let child = path
                        .strip_prefix(&prefix)
                        .is_some_and(|rest| !rest.contains('/'));
                    child && matches!(class.as_str(), "CollisionShape2D" | "CollisionPolygon2D")
                })
                .map(|(path, _)| path.clone())
                .collect()
        } else {
            vec![from.clone()]
        };
        if holders.is_empty() {
            self.notes.push(format!(
                "connection `{signal}` from `{from}`: it has no shape child to report it"
            ));
        }
        // A Godot method every node has is a verb a row already knows.
        let (action, value) = match method {
            "show" => ("visible", Toml::Boolean(true)),
            "hide" => ("visible", Toml::Boolean(false)),
            "queue_free" => ("free", Toml::String(String::new())),
            _ => ("call", Toml::String(method.to_string())),
        };
        for holder in holders {
            self.bind(&holder, &to, &event, action, value.clone());
        }
    }

    /// One binding row on `holder`, aimed at `to`.
    fn bind(&mut self, holder: &str, to: &str, event: &str, action: &str, value: Toml) {
        let mut row = toml::Table::new();
        row.insert("event".into(), Toml::String(event.into()));
        row.insert("action".into(), Toml::String(action.into()));
        row.insert("target".into(), Toml::String(relative(holder, to)));
        row.insert("value".into(), value);
        let Some(table) = self.table(holder) else {
            return;
        };
        if event.starts_with("collision")
            && let Some(Toml::Table(collider)) = table.get_mut("collider2d")
        {
            collider.insert(
                "events".into(),
                Toml::Array(vec![Toml::String("collision".into())]),
            );
        }
        let rows = table
            .entry("bindings")
            .or_insert_with(|| Toml::Array(Vec::new()));
        if let Toml::Array(rows) = rows {
            rows.push(Toml::Table(row));
        }
    }

    /// An AnimationPlayer's clips, written beside the scene and named by its
    /// `animation` component.
    fn player(&mut self, section: &Section, stem: &str, files: &mut Vec<(String, String)>) {
        let name = section.attr_str("name").unwrap_or("AnimationPlayer");
        let parent = section
            .attr_str("parent")
            .map(|p| if p == "." { "" } else { p });
        let path = match parent {
            None => String::new(),
            Some("") => name.to_string(),
            Some(p) => format!("{p}/{name}"),
        };
        let Some(clips) =
            crate::godot::anim::convert(section, &path, &self.classes, &self.res)
        else {
            return;
        };
        // `animations/` is where a clip library lives; the scene's own path
        // goes in the name so two scenes' players never share a file.
        let file = format!("animations/{}_{}.toml", slug(stem), slug(&path));
        let root = section
            .field("root_node")
            .and_then(node_path)
            .unwrap_or_else(|| "..".to_string());
        let autoplay = section
            .field("autoplay")
            .and_then(Value::as_str)
            .map(str::to_string);
        for note in &clips.notes {
            self.notes
                .push(format!("`{path}` (AnimationPlayer): {note}"));
        }
        let mut missing = None;
        files.push((file.clone(), clips.toml.clone()));
        let Some(table) = self.table(&path) else {
            return;
        };
        let animation = table
            .entry("animation")
            .or_insert_with(|| Toml::Table(toml::Table::new()));
        if let Toml::Table(animation) = animation {
            animation.insert("library".into(), Toml::String(file));
            animation.insert("root".into(), Toml::String(root));
            if let Some(clip) = autoplay.filter(|c| !c.is_empty()) {
                if clips.names.contains(&clip) {
                    animation.insert("autoplay".into(), Toml::String(clip));
                } else {
                    missing = Some(clip);
                }
            }
        }
        if let Some(clip) = missing {
            self.notes.push(format!(
                "`{path}` (AnimationPlayer): autoplays `{clip}`, which its libraries do not have; Godot ignores it silently"
            ));
        }
    }

    /// An AnimationTree's state machine, written beside the scene and run by
    /// its `state_machine` component against the player it names.
    fn machine(&mut self, section: &Section, stem: &str, files: &mut Vec<(String, String)>) {
        let name = section.attr_str("name").unwrap_or("AnimationTree");
        let path = match section.attr_str("parent") {
            None => String::new(),
            Some(".") => name.to_string(),
            Some(p) => format!("{p}/{name}"),
        };
        let Some(machine) = crate::godot::machine::convert(section, &self.res) else {
            if section.field("tree_root").is_some() {
                self.notes.push(format!(
                    "`{path}` (AnimationTree): its root is not a state machine; blend trees have no equivalent"
                ));
            }
            return;
        };
        for note in &machine.notes {
            self.notes.push(format!("`{path}` (AnimationTree): {note}"));
        }
        let file = format!("animations/{}_{}_machine.toml", slug(stem), slug(&path));
        files.push((file.clone(), machine.toml));
        // Its own libraries make the tree its own player; otherwise it drives
        // the one `anim_player` names.
        let own = section
            .fields
            .iter()
            .any(|(key, _)| key.starts_with("libraries"));
        let player = if own {
            String::new()
        } else {
            section
                .field("anim_player")
                .and_then(node_path)
                .unwrap_or_default()
        };
        let active = section.field("active") != Some(&Value::Bool(false));
        let Some(table) = self.table(&path) else {
            return;
        };
        let mut component = toml::Table::new();
        component.insert("machine".into(), Toml::String(file));
        component.insert("player".into(), Toml::String(player));
        component.insert("active".into(), Toml::Boolean(active));
        table.insert("state_machine".into(), Toml::Table(component));
    }

    /// Add an inline asset, or find the one already added with the same
    /// contents, and answer its id: two layers over one tileset share it.
    fn asset(&mut self, mut table: toml::Table, id: &str, component: &str, index: usize) -> String {
        let key = toml::to_string(&table).unwrap_or_default();
        if let Some(found) = self.asset_ids.get(&key) {
            return found.clone();
        }
        let reference = if index == 0 {
            format!("{id}_{component}")
        } else {
            format!("{id}_{component}_{index}")
        };
        table.insert("id".into(), Toml::String(reference.clone()));
        self.assets.push(Toml::Table(table));
        self.asset_ids.insert(key, reference.clone());
        reference
    }

    /// What a node's `parent` names: the id of a node this file declares, or
    /// for one added inside an instance, the path of names from the scene's
    /// root, which the loader walks through the instance.
    fn parent_ref(&self, parent: Option<&str>) -> String {
        let Some(parent) = parent else {
            return String::new();
        };
        if let Some(id) = self.ids.get(parent) {
            return id.clone();
        }
        let root = self.nodes.first().and_then(|n| n.get("name")).and_then(Toml::as_str);
        match root {
            Some(root) => format!("{root}/{parent}"),
            None => String::new(),
        }
    }

    /// A readable id for a scene path, unique within this file.
    fn id_for(&mut self, path: &str, name: &str) -> String {
        let base = if path.is_empty() {
            format!("n_{}", slug(name))
        } else {
            format!("n_{}", slug(path))
        };
        let mut id = base.clone();
        let mut n = 2;
        while self.taken.contains(&id) {
            id = format!("{base}_{n}");
            n += 1;
        }
        self.taken.push(id.clone());
        self.ids.insert(path.to_string(), id.clone());
        id
    }
}

/// The binding event a Godot signal is here: a click, a collision or a
/// pointer crossing where one says the same thing, else the name the node
/// emits, a widget's change and submit included.
fn event_of(signal: &str, control: bool, handler: Option<&str>) -> String {
    match signal {
        "body_entered" | "area_entered" => "collision_start".into(),
        "body_exited" | "area_exited" => "collision_stop".into(),
        "mouse_entered" => "pointer_enter".into(),
        "mouse_exited" => "pointer_exit".into(),
        "pressed" | "button_up" if control => "pointer_click".into(),
        _ => {
            let emitted = match handler {
                Some("on_change") => CHANGE_EVENT,
                Some("on_submit") => SUBMIT_EVENT,
                _ => signal,
            };
            format!("emitted:{emitted}")
        }
    }
}

/// A scene an instance names, read far enough to know its root and its
/// nodes' classes. `None` when the file is missing or will not parse.
fn outline(res: &Resources<'_>, prefab: &str) -> Option<Outline> {
    let text = std::fs::read_to_string(res.root.join(prefab)).ok()?;
    let document = crate::godot::parse(&text).ok()?;
    let own = crate::godot::nodes::resources_of(&document, res.root, res.project);
    let mut classes = BTreeMap::new();
    let mut instances = BTreeMap::new();
    let mut scripts = BTreeMap::new();
    for section in document.each("node") {
        let name = section.attr_str("name").unwrap_or_default();
        let path = match section.attr_str("parent") {
            None => String::new(),
            Some(".") => name.to_string(),
            Some(p) => format!("{p}/{name}"),
        };
        if let Some(nested) = section.attr("instance").and_then(|i| own.path(i)) {
            instances.insert(path.clone(), nested.to_string());
        }
        if let Some(script) = section.field("script").and_then(|s| own.path(s)) {
            scripts.insert(path.clone(), script.to_string());
        }
        let class = section
            .attr_str("type")
            .map(str::to_string)
            .unwrap_or_default();
        classes.insert(path, class);
    }
    Some(Outline {
        classes,
        scripts,
        instances,
    })
}

/// The class of the node `inner` names inside `prefab`, looking through the
/// prefabs it instances in turn; empty when nothing there declares one.
fn class_in(res: &Resources<'_>, prefab: &Outline, inner: &str, depth: usize) -> String {
    find_in(res, prefab, inner, depth, &|o, p| o.classes.get(p).filter(|c| !c.is_empty()).cloned())
        .unwrap_or_default()
}

/// The Godot script the node `inner` names inside `prefab` carries.
fn script_in(res: &Resources<'_>, prefab: &Outline, inner: &str) -> Option<String> {
    find_in(res, prefab, inner, 0, &|o, p| o.scripts.get(p).cloned())
}

/// What `pick` reads off the node `inner` names inside `prefab`, following
/// the path into each prefab it instances on the way down.
fn find_in(
    res: &Resources<'_>,
    prefab: &Outline,
    inner: &str,
    depth: usize,
    pick: &dyn Fn(&Outline, &str) -> Option<String>,
) -> Option<String> {
    // A prefab whose root is an instance has that scene's nodes as its own,
    // and a node this file did not declare with a type is one of those.
    let first = inner.split('/').next().unwrap_or_default();
    let declared = prefab.classes.get(first).is_some_and(|c| !c.is_empty())
        || prefab.instances.contains_key(first);
    if !declared
        && !inner.is_empty()
        && let Some(nested) = prefab.instances.get("").filter(|_| depth < 8)
        && let Some(nested) = outline(res, nested)
    {
        return find_in(res, &nested, inner, depth + 1, pick);
    }
    if let Some(found) = pick(prefab, inner) {
        return Some(found);
    }
    let segments: Vec<&str> = inner.split('/').collect();
    let mut walked = String::new();
    for (index, segment) in segments.iter().enumerate() {
        walked = if walked.is_empty() {
            (*segment).to_string()
        } else {
            format!("{walked}/{segment}")
        };
        // The node itself, when it is a prefab's root, or something inside one.
        let Some(nested) = prefab.instances.get(&walked) else {
            continue;
        };
        // Prefabs nest a handful deep at most; a cycle is Godot's error too.
        let nested = (depth < 8).then(|| outline(res, nested)).flatten()?;
        let rest = segments[index + 1..].join("/");
        return find_in(res, &nested, &rest, depth + 1, pick);
    }
    None
}

/// The node path from one scene path to another, as a binding's `target`.
fn relative(from: &str, to: &str) -> String {
    let a: Vec<&str> = from.split('/').filter(|p| !p.is_empty()).collect();
    let b: Vec<&str> = to.split('/').filter(|p| !p.is_empty()).collect();
    let shared = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<&str> = vec![".."; a.len() - shared];
    parts.extend(&b[shared..]);
    if parts.is_empty() {
        return String::new();
    }
    parts.join("/")
}

fn slug(text: &str) -> String {
    let slug: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if slug.is_empty() {
        "root".to_string()
    } else {
        slug
    }
}
