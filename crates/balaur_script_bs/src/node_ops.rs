// The engine's node operations as a script types them, read from their
// signatures in `balaur_core::node_api::NODE_OPS`.

use crate::doc_types::{self, Cx};
use balaur_bs::{HostSignature, Ty};
use balaur_core::node_api::NODE_OPS;
use std::collections::BTreeSet;

/// What a key names when it is a node operation: `node.get_node`, or
/// `node.transform.translate` for one its component drives.
pub(crate) const PREFIX: &str = "node.";
/// The binding group the engine installs the operations into, docs and all.
pub(crate) const MODULE: &str = "node";

/// The operation a key calls: its last segment.
pub(crate) fn op_name(key: &str) -> Option<&str> {
    key.strip_prefix(PREFIX)?.rsplit('.').next()
}

/// An argument the engine takes as a table, or a payload, is an `any`, and may
/// be left out at the end, as a `T?` may.
pub(crate) fn signatures() -> Vec<HostSignature> {
    let cx = Cx {
        module: MODULE,
        records: &BTreeSet::new(),
    };
    NODE_OPS
        .iter()
        .map(|op| {
            let (params, ret) = doc_types::signature(op.signature, &cx).unwrap_or_else(|| {
                panic!(
                    "`{}` has an unreadable signature: {}",
                    op.name, op.signature
                )
            });
            HostSignature {
                key: match op.acts_on.first() {
                    Some(comp) => format!("{PREFIX}{comp}.{}", op.name),
                    None => format!("{PREFIX}{}", op.name),
                },
                params: std::iter::once(Ty::Node).chain(params).collect(),
                ret,
                is_async: false,
            }
        })
        .collect()
}
