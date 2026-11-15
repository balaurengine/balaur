// A `pub` field of a script's class set to a node's path: it holds that
// node's own object, kept so a free leaves nil and a reload points it again.

use super::{BsHost, File, Instance};
use balaur_bs::Ty;
use balaur_bs::runtime::Value as L;
use balaur_script::{NodeId, Value};
use std::rc::Rc;

/// One such field: the node holding it, the field, the path its scene gave,
/// and the node it found, while that node stands.
pub(super) struct Link {
    pub(super) holder: u64,
    pub(super) field: String,
    path: String,
    target: Option<u64>,
}

/// The script's class a field holds, when its type is one or `T?` of one.
pub(super) fn class_of(file: &File, ty: &Ty) -> Option<String> {
    let class = match ty {
        Ty::Optional(inner) => &**inner,
        other => other,
    };
    match class {
        Ty::Struct(name) if super::exports::is_script_class(&file.compiled.meta, name) => {
            Some(name.clone())
        }
        _ => None,
    }
}

impl BsHost {
    /// Note a field set to `path`, pointed when its holder's `init` is due.
    pub(super) fn note_link(&self, holder: u64, field: &str, path: &str) {
        let mut links = self.inner.links.borrow_mut();
        links.retain(|link| !(link.holder == holder && link.field == field));
        links.push(Link {
            holder,
            field: field.to_string(),
            path: path.to_string(),
            target: None,
        });
    }

    /// Point every field `holder` holds by path, before its `init` reads one.
    pub(super) fn point_links_of(&self, holder: u64) {
        let count = self.inner.links.borrow().len();
        for at in 0..count {
            if self
                .inner
                .links
                .borrow()
                .get(at)
                .is_some_and(|l| l.holder == holder)
            {
                self.point(at, true);
            }
        }
    }

    /// Point every field again: a reload made new objects, or a restore
    /// respawned a target.
    pub(super) fn point_all_links(&self) {
        let count = self.inner.links.borrow().len();
        for at in 0..count {
            self.point(at, false);
        }
    }

    /// A node going: the fields it held are forgotten, and each field holding
    /// it is nil until its path finds a node again.
    pub(super) fn unlink(&self, node: u64) {
        self.inner
            .links
            .borrow_mut()
            .retain(|link| link.holder != node);
        let held: Vec<(u64, String)> = self
            .inner
            .links
            .borrow_mut()
            .iter_mut()
            .filter(|link| link.target == Some(node))
            .map(|link| {
                link.target = None;
                (link.holder, link.field.clone())
            })
            .collect();
        for (holder, field) in held {
            if let Some((file, inst)) = self.instance(NodeId(holder)) {
                clear(&file, &inst, &field);
            }
        }
    }

    /// Whether `field` of `holder` is one the scene set by path, which a
    /// snapshot neither saves nor restores: the next pointing sets it.
    pub(super) fn is_link(&self, holder: u64, field: &str) -> bool {
        self.inner
            .links
            .borrow()
            .iter()
            .any(|link| link.holder == holder && link.field == field)
    }

    // The link at `at` pointed at what its path finds now; `say` warns when
    // the path finds no node of the field's class.
    fn point(&self, at: usize, say: bool) {
        let Some((holder, field, path)) = self
            .inner
            .links
            .borrow()
            .get(at)
            .map(|l| (l.holder, l.field.clone(), l.path.clone()))
        else {
            return;
        };
        let Some((file, inst)) = self.instance(NodeId(holder)) else {
            return;
        };
        let found = balaur_core::entity_of(NodeId(holder))
            .ok()
            .and_then(|from| {
                balaur_core::scene::find_node(&self.inner.engine.world(), from, &path)
            });
        let Some(target) = found.map(|e| balaur_core::node_id_of(e).0) else {
            if say {
                tracing::warn!(
                    "[{}] property '{field}' names no node at '{path}'",
                    inst.path
                );
            }
            return;
        };
        let class = file.field_ty(&field).and_then(|ty| class_of(&file, &ty));
        let wanted = class
            .as_deref()
            .and_then(|class| file.compiled.meta.class_file(class));
        let pointer = self.instance(NodeId(target)).and_then(|(tfile, tinst)| {
            (Some(tinst.path.as_str()) == wanted && tfile.place == file.place && !tinst.gone.get())
                .then(|| tinst.pointer.get())
        });
        let Some(pointer) = pointer else {
            if say {
                let class = class.unwrap_or_default();
                tracing::warn!(
                    "[{}] property '{field}': the node at '{path}' runs no `{class}`",
                    inst.path
                );
            }
            return;
        };
        let mut guard = inst.obj.borrow_mut();
        let Some(obj) = guard.as_mut() else {
            return;
        };
        let set = match file.home.rt.try_borrow_mut() {
            Ok(mut rt) => rt.set_object(obj, &field, pointer),
            Err(_) => file.home.reentry.set_object(obj, &field, pointer),
        };
        drop(guard);
        match set {
            Ok(()) => {
                if let Some(link) = self.inner.links.borrow_mut().get_mut(at) {
                    link.target = Some(target);
                }
            }
            Err(e) => tracing::error!("[{}] property '{field}': {e}", inst.path),
        }
    }
}

// Nil in a `T?` field; a field of the class itself keeps what it holds.
fn clear(file: &Rc<File>, inst: &Rc<Instance>, field: &str) {
    if !matches!(file.field_ty(field), Some(Ty::Optional(_))) {
        return;
    }
    let mut guard = inst.obj.borrow_mut();
    let Some(obj) = guard.as_mut() else {
        return;
    };
    let cleared = match file.home.rt.try_borrow_mut() {
        Ok(mut rt) => rt.set(obj, field, &L::Void),
        Err(_) => file.home.reentry.set(obj, field, &L::Void),
    };
    if let Err(e) = cleared {
        tracing::error!("[{}] property '{field}': {e}", inst.path);
    }
}

/// The scene's value for a field: a path string, for a field of a class.
pub(super) fn path_of(v: &Value) -> Option<&str> {
    match v {
        Value::Str(path) => Some(path),
        _ => None,
    }
}
