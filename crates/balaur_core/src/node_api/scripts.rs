//! A node's script as the node ops reach it: which file it runs, what it
//! sets, calling it, and attaching, detaching and freeing it.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module above, split for length"
)]
use super::*;

pub(super) fn script_path(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    let path = world
        .get::<&NodeScript>(e)
        .ok()
        .map(|script| script.path.clone());
    drop(world);
    Ok(path.map_or(Value::Nil, |path| Value::Str(spelled(eng, e, path))))
}

pub(super) fn script_props(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let world = eng.world();
    Ok(Value::Map(
        world
            .get::<&NodeScript>(e)
            .map(|script| script.props.clone())
            .unwrap_or_default(),
    ))
}

pub(super) fn set_script(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let props = match args.get(2) {
        None | Some(Value::Nil) => Vec::new(),
        Some(Value::Map(fields)) => fields.clone(),
        Some(other) => bail!(
            "set_script props must be a table, got {}",
            other.type_name()
        ),
    };
    crate::changes::scripted(eng, e);
    match args.get(1) {
        None | Some(Value::Nil) => scene::name_script(eng, e, None),
        Some(_) => {
            let path = rooted(eng, e, text(args, 1)?);
            scene::name_script(eng, e, Some((&path, &props)));
        }
    }
    Ok(Value::Nil)
}

pub(super) fn instance(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let source = eng
        .world()
        .get::<&crate::scene_toml::Instanced>(e)
        .ok()
        .map(|instanced| instanced.source.clone());
    Ok(source.map_or(Value::Nil, |source| Value::Str(spelled(eng, e, source))))
}

/// A path read off a node, as the project whose content it plays spells it.
fn spelled(eng: &Engine, e: Entity, path: String) -> String {
    let mut value = toml::Value::String(path);
    crate::document_paths::read_off(eng, e, &mut value);
    match value {
        toml::Value::String(path) => path,
        _ => unreachable!("a string stays a string"),
    }
}

/// A path written onto a node, made absolute when it plays another project.
fn rooted(eng: &Engine, e: Entity, path: &str) -> String {
    match crate::document_paths::written_on(eng, e, &toml::Value::String(path.to_string())) {
        Some(toml::Value::String(path)) => path,
        _ => path.to_string(),
    }
}

/// Whether the node's script declares a method, which `call` cannot say: it
/// answers nil both for a method that is not there and for one that returned
/// nothing.
pub(super) fn has_method(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let method = text(args, 1)?;
    Ok(Value::Bool(eng.script_host().is_some_and(|host| {
        host.has_method(crate::node_id_of(e), method)
    })))
}

pub(super) fn script_field(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let name = text(args, 1)?;
    Ok(eng
        .script_host()
        .and_then(|host| host.script_field(crate::node_id_of(e), name))
        .unwrap_or(Value::Nil))
}

/// `node:call("method", ...)`: one script calling another's method, with
/// the target's return value coming back. Nil when the node has no script,
/// no such method (handlers are opt-in), or the method suspended on an
/// await; the call itself runs to completion before this returns, so a
/// handler may spawn, free or call further nodes.
pub(super) fn call(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let method = text(args, 1)?;
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    Ok(host
        .call_on(crate::node_id_of(e), method, args.get(2..).unwrap_or(&[]))
        .unwrap_or(Value::Nil))
}

/// `node.call_async`: a token woken with the method's result once it has
/// returned, on the next step at the earliest so the caller has parked.
pub(super) fn call_async(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let method = text(args, 1)?;
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    let token = eng.next_token();
    let rest = args.get(2..).unwrap_or(&[]);
    if let Some(result) = host.call_on_async(crate::node_id_of(e), method, rest, token) {
        crate::timers::wake_next_step(eng, token, result);
    }
    Ok(Value::Int(i64::try_from(token).unwrap_or(i64::MAX)))
}

pub(super) fn emit(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let name = text(args, 1)?.to_string();
    let payload = args.get(2).cloned().unwrap_or(Value::Nil);
    crate::events::emit_from(eng, e, &name, payload);
    Ok(Value::Nil)
}

/// `node:attach_script(path, props)`: the scene's `script` key, at run time.
/// `props` is optional and holds what this node overrides of the script's
/// exported defaults, so a spawned node is tuned the way an authored one is.
pub(super) fn attach_script(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let props = match args.get(2) {
        None | Some(Value::Nil) => Vec::new(),
        Some(Value::Map(fields)) => fields.clone(),
        Some(other) => bail!(
            "attach_script props must be a table, got {}",
            other.type_name()
        ),
    };
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    crate::parked::refuse_if_parked(eng, e, "node.attach_script")?;
    let path = rooted(eng, e, text(args, 1)?);
    crate::changes::scripted(eng, e);
    scene::name_script(eng, e, Some((&path, &props)));
    host.attach_with_props(crate::node_id_of(e), &path, &props)?;
    Ok(Value::Nil)
}

/// `node:detach_script()`: drop the instance, keeping the node.
///
/// A tool that tears a running scene down needs the scripts to stop before
/// the world does: an instance whose next `update` runs against a world its
/// own components have left throws where nothing is wrong.
pub(super) fn detach_script(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let host = eng
        .script_host()
        .ok_or_else(|| anyhow!("no script backend is running"))?;
    host.detach(crate::node_id_of(e));
    Ok(Value::Nil)
}

pub(super) fn queue_free(eng: &Engine, args: &[Value]) -> Result<Value> {
    eng.defer_free(node(args)?);
    Ok(Value::Nil)
}

pub(super) fn free(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    scene::announce_leaving(eng, &[e]);
    scene::free_node(eng, e);
    Ok(Value::Nil)
}

pub(super) fn authored(eng: &Engine, args: &[Value]) -> Result<Value> {
    let e = node(args)?;
    let name = text(args, 1)?;
    if !crate::components::has(eng, e, name) {
        return Ok(Value::Nil);
    }
    let mut written = crate::components::written(eng, e, name)
        .unwrap_or_else(|| toml::Value::Table(toml::Table::new()));
    crate::document_paths::read_off(eng, e, &mut written);
    from_toml(&written)
}
