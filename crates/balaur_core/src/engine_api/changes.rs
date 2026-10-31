//! What a script asks of the change ticks (PLAN-ecs §2): the tick now, and
//! the nodes a component went onto, was written on or came off after one.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module above, split for length"
)]
use super::*;

pub(super) fn change_tick(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Int(
        i64::try_from(eng.change_tick()).unwrap_or(i64::MAX),
    ))
}

/// The registry index the component named by argument 0 sits at, and the
/// change tick argument 1 asks from.
fn since_args(eng: &Engine, args: &[Value]) -> Result<(usize, u64)> {
    let name = text(args, 0)?;
    let index = crate::components::index_of(eng, name)
        .ok_or_else(|| anyhow!("no component is registered as {name}"))?;
    Ok((index, u64::try_from(integer(args, 1)?).unwrap_or(0)))
}

fn nodes_value(nodes: Vec<hecs::Entity>) -> Value {
    Value::List(
        nodes
            .into_iter()
            .map(|node| Value::Node(crate::node_id_of(node).0))
            .collect(),
    )
}

pub(super) fn added_since(eng: &Engine, args: &[Value]) -> Result<Value> {
    let (index, since) = since_args(eng, args)?;
    Ok(nodes_value(crate::components::ticks::added_since(
        eng, index, since,
    )))
}

pub(super) fn changed_since(eng: &Engine, args: &[Value]) -> Result<Value> {
    let (index, since) = since_args(eng, args)?;
    Ok(nodes_value(crate::components::ticks::changed_since(
        eng, index, since,
    )))
}

pub(super) fn removed_since(eng: &Engine, args: &[Value]) -> Result<Value> {
    let (index, since) = since_args(eng, args)?;
    Ok(nodes_value(crate::components::ticks::removed_since(
        eng, index, since,
    )))
}
