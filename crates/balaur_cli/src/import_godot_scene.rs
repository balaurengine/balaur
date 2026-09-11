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

use crate::import_godot::{Document, Section, Value};
use crate::import_godot_anim::{join, node_path};
use crate::import_godot_nodes::{Family, Mapped, Resources, family, map};

/// A converted scene, the files written beside it, and what did not carry.
pub(crate) struct Converted {
    pub scene_toml: String,
    /// Clip libraries, by project path.
    pub files: Vec<(String, String)>,
    pub notes: Vec<String>,
}

/// What an instance of a scene needs to know about it: its root's name, and
/// every node's class by its path under that root.
struct Outline {
    root: String,
    classes: BTreeMap<String, String>,
}

/// `res://a/b.tscn` as `a/b.tscn`.
pub(crate) fn project_path(reference: &str) -> String {
    reference
        .strip_prefix("res://")
        .unwrap_or(reference)
        .to_string()
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

struct Walk<'a> {
    res: Resources<'a>,
    nodes: Vec<toml::Table>,
    assets: Vec<Toml>,
    slots: BTreeMap<String, Slot>,
    classes: BTreeMap<String, String>,
    instances: BTreeMap<String, Outline>,
    ids: BTreeMap<String, String>,
    taken: Vec<String>,
    notes: Vec<String>,
}

/// Convert one scene. `path` is its project path, for naming the files it
/// writes beside itself; `root` is the Godot project's root.
pub(crate) fn convert(document: &Document, path: &str, root: &Path) -> Result<Converted> {
    let mut walk = Walk {
        res: resources(document, root),
        nodes: Vec::new(),
        assets: Vec::new(),
        slots: BTreeMap::new(),
        classes: BTreeMap::new(),
        instances: BTreeMap::new(),
        ids: BTreeMap::new(),
        taken: Vec::new(),
        notes: Vec::new(),
    };
    let nodes: Vec<&Section> = document.each("node").collect();
    for section in &nodes {
        walk.node(section);
    }
    for connection in document.each("connection") {
        walk.connection(connection);
    }
    let mut files = Vec::new();
    let stem = path.strip_suffix(".tscn").unwrap_or(path);
    for section in &nodes {
        if section.attr_str("type") != Some("AnimationPlayer") {
            continue;
        }
        walk.player(section, stem, &mut files);
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

fn resources<'a>(document: &Document, root: &'a Path) -> Resources<'a> {
    let mut external = BTreeMap::new();
    for section in document.each("ext_resource") {
        let (Some(id), Some(path)) = (section.attr_str("id"), section.attr_str("path")) else {
            continue;
        };
        let kind = section.attr_str("type").unwrap_or_default().to_string();
        external.insert(id.to_string(), (kind, project_path(path)));
    }
    let internal = document
        .each("sub_resource")
        .filter_map(|s| Some((s.attr_str("id")?.to_string(), s.clone())))
        .collect();
    Resources {
        external,
        internal,
        root,
    }
}

impl Walk<'_> {
    fn node(&mut self, section: &Section) {
        let name = section.attr_str("name").unwrap_or("Node").to_string();
        let parent = section.attr_str("parent").map(|p| if p == "." { "" } else { p });
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
        let parent_id = parent
            .and_then(|p| self.ids.get(p))
            .cloned()
            .unwrap_or_default();
        table.insert("parent".into(), Toml::String(parent_id));
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
            self.notes.push(format!("`{path}` instances a scene this file does not declare"));
            return;
        };
        let outline = outline(&self.res, &prefab);
        let id = self.id_for(path, name);
        let mut table = toml::Table::new();
        table.insert("id".into(), Toml::String(id));
        table.insert("name".into(), Toml::String(name.to_string()));
        let parent_id = parent
            .and_then(|p| self.ids.get(p))
            .cloned()
            .unwrap_or_default();
        table.insert("parent".into(), Toml::String(parent_id));
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
            path: outline.root.clone(),
        };
        self.slots.insert(path.to_string(), root);
        self.classes.insert(path.to_string(), class.clone());
        let mapped = map(&class, section, parent_class, &self.res);
        let id = format!("{}_root", self.ids[path]);
        self.write(path, &class, mapped, &id);
        self.script(section, path);
        self.instances.insert(path.to_string(), outline);
    }

    /// A node with no type: an edit of one an instance above it already has.
    fn edit(&mut self, section: &Section, path: &str, parent_class: &str) {
        let owner = self
            .instances
            .keys()
            .filter(|inst| path.starts_with(&format!("{inst}/")))
            .max_by_key(|inst| inst.len())
            .cloned();
        let Some(owner) = owner else {
            self.notes.push(format!(
                "`{path}` has no type and sits in no instance; skipped"
            ));
            return;
        };
        let outline = &self.instances[&owner];
        let inner = &path[owner.len() + 1..];
        let class = outline.classes.get(inner).cloned().unwrap_or_default();
        let override_path = format!("{}/{inner}", outline.root);
        let Slot::Override { instance, .. } = self.slots[&owner].clone() else {
            return;
        };
        self.slots.insert(
            path.to_string(),
            Slot::Override {
                instance,
                path: override_path,
            },
        );
        self.classes.insert(path.to_string(), class.clone());
        let mapped = map(&class, section, parent_class, &self.res);
        let id = format!("{}_{}", self.ids[&owner], slug(inner));
        self.write(path, &class, mapped, &id);
        self.script(section, path);
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

    /// One mapped node into its table, its inline meshes into the scene's
    /// `[[assets]]`, and its notes prefixed with where they came from.
    fn write(&mut self, path: &str, class: &str, mapped: Mapped, id: &str) {
        let Mapped {
            keys,
            mut components,
            assets,
            notes,
        } = mapped;
        for (index, (component, mut mesh)) in assets.into_iter().enumerate() {
            let asset = if index == 0 {
                format!("{id}_{component}")
            } else {
                format!("{id}_{component}_{index}")
            };
            mesh.insert("id".into(), Toml::String(asset.clone()));
            self.assets.push(Toml::Table(mesh));
            if let Some(Toml::Table(table)) = components.get_mut(component) {
                table.insert("mesh".into(), Toml::String(format!("#{asset}")));
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
            self.notes.push(format!("`{path}`: an inline script is not converted"));
            return;
        };
        let exports = exported(&self.res.root.join(&godot));
        let mut props = toml::Table::new();
        for (key, value) in &section.fields {
            if exports.contains(key) {
                if let Some(value) = toml_of(value, &self.res) {
                    props.insert(key.clone(), value);
                }
            }
        }
        let mut script = toml::Table::new();
        script.insert("source".into(), Toml::String(script_path(&godot)));
        if !props.is_empty() {
            script.insert("props".into(), Toml::Table(props));
        }
        if let Some(table) = self.table(path) {
            table.insert("script".into(), Toml::Table(script));
        }
    }

    /// A signal connection as the handler key a widget names, or a `call`
    /// binding, or a note when neither can say it.
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
        // A widget handler runs on the widget's node or the nearest scripted
        // ancestor, so it can say a connection to either and nothing else.
        let upward = to.is_empty() || from == to || from.starts_with(&format!("{to}/"));
        let handler = match signal {
            "pressed" | "button_up" => Some("on_click"),
            "toggled" | "text_changed" | "value_changed" | "item_selected" | "folding_changed" => {
                Some("on_change")
            }
            "text_submitted" => Some("on_submit"),
            "focus_entered" => Some("on_focus"),
            _ => None,
        };
        if family(&class) == Family::Control && upward {
            if let Some(handler) = handler {
                if let Some(Toml::Table(widget)) = self
                    .table(&from)
                    .map(|t| t.entry("widget").or_insert_with(|| Toml::Table(toml::Table::new())))
                {
                    widget.insert(handler.into(), Toml::String(method.to_string()));
                }
                return;
            }
        }
        let event = match signal {
            "body_entered" | "area_entered" => Some("collision_start"),
            "body_exited" | "area_exited" => Some("collision_stop"),
            "mouse_entered" => Some("pointer_enter"),
            "mouse_exited" => Some("pointer_exit"),
            _ => None,
        };
        let Some(event) = event else {
            self.notes.push(format!(
                "connection `{signal}` from `{from}` to `{method}` on `{to}`: subscribe to it in a script"
            ));
            return;
        };
        let mut row = toml::Table::new();
        row.insert("event".into(), Toml::String(event.into()));
        row.insert("action".into(), Toml::String("call".into()));
        row.insert("target".into(), Toml::String(relative(&from, &to)));
        row.insert("value".into(), Toml::String(method.to_string()));
        if let Some(table) = self.table(&from) {
            let rows = table
                .entry("bindings")
                .or_insert_with(|| Toml::Array(Vec::new()));
            if let Toml::Array(rows) = rows {
                rows.push(Toml::Table(row));
            }
        }
    }

    /// An AnimationPlayer's clips, written beside the scene and named by its
    /// `animation` component.
    fn player(&mut self, section: &Section, stem: &str, files: &mut Vec<(String, String)>) {
        let name = section.attr_str("name").unwrap_or("AnimationPlayer");
        let parent = section.attr_str("parent").map(|p| if p == "." { "" } else { p });
        let path = match parent {
            None => String::new(),
            Some("") => name.to_string(),
            Some(p) => format!("{p}/{name}"),
        };
        let Some(clips) = crate::import_godot_anim::convert(section, &path, &self.classes, &self.res)
        else {
            return;
        };
        let file = format!("{stem}_{}.anim.toml", slug(&path));
        let root = section
            .field("root_node")
            .and_then(node_path)
            .unwrap_or_else(|| "..".to_string());
        let autoplay = section.field("autoplay").and_then(Value::as_str).map(str::to_string);
        for note in clips.notes {
            self.notes.push(format!("`{path}` (AnimationPlayer): {note}"));
        }
        files.push((file.clone(), clips.toml));
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
                animation.insert("autoplay".into(), Toml::String(clip));
            }
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

/// A scene an instance names, read far enough to know its root and its
/// nodes' classes. `None` when the file is missing or will not parse.
fn outline(res: &Resources<'_>, prefab: &str) -> Option<Outline> {
    let text = std::fs::read_to_string(res.root.join(prefab)).ok()?;
    let document = crate::import_godot::parse(&text).ok()?;
    let mut classes = BTreeMap::new();
    let mut root = String::new();
    for section in document.each("node") {
        let name = section.attr_str("name").unwrap_or_default();
        let path = match section.attr_str("parent") {
            None => {
                root = name.to_string();
                String::new()
            }
            Some(".") => name.to_string(),
            Some(p) => format!("{p}/{name}"),
        };
        // An instance inside the prefab: its class is its own prefab's root,
        // which one level down is as far as an override usually reaches.
        let class = section
            .attr_str("type")
            .map(str::to_string)
            .unwrap_or_default();
        classes.insert(path, class);
    }
    Some(Outline { root, classes })
}

/// The `@export` names a GDScript file declares.
fn exported(file: &Path) -> Vec<String> {
    let Ok(source) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    let mut pending = false;
    for line in source.lines() {
        let line = line.trim();
        let exporting = line.starts_with("@export");
        if exporting || pending {
            if let Some(at) = line.find("var ") {
                let name: String = line[at + 4..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    names.push(name);
                }
                pending = false;
            } else {
                // `@export` alone on a line annotates the `var` on the next.
                pending = exporting;
            }
        }
    }
    names
}

/// A Godot value as the TOML a script's props take.
fn toml_of(value: &Value, res: &Resources<'_>) -> Option<Toml> {
    Some(match value {
        Value::Null => return None,
        Value::Bool(b) => Toml::Boolean(*b),
        Value::Int(n) => Toml::Integer(*n),
        Value::Float(n) => Toml::Float(*n),
        Value::Str(s) | Value::Name(s) => Toml::String(s.clone()),
        Value::Array(items) => Toml::Array(items.iter().filter_map(|v| toml_of(v, res)).collect()),
        Value::Dict(pairs) => {
            let mut table = toml::Table::new();
            for (key, value) in pairs {
                if let (Some(key), Some(value)) = (key.as_str(), toml_of(value, res)) {
                    table.insert(key.to_string(), value);
                }
            }
            Toml::Table(table)
        }
        Value::Call { name, args } => match name.as_str() {
            "NodePath" => Toml::String(args.first()?.as_str()?.to_string()),
            "ExtResource" => Toml::String(scene_path(res.path(value)?)),
            "SubResource" => return None,
            _ => Toml::Array(args.iter().filter_map(|v| toml_of(v, res)).collect()),
        },
        Value::Object { .. } => return None,
    })
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
