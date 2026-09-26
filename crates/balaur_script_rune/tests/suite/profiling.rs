//! What profiling says a function cost: its own instructions and its calls,
//! counted by the VM, so a function a script calls from its `update` is its
//! own row rather than part of the file's.

use super::backend::{app_in, project, spawn};

#[test]
fn a_helper_an_update_calls_is_priced_by_its_own_calls() {
    let dir = project(&[(
        "busy.rn",
        "fn helper(n) { n + 1 }\n\
         pub fn update(this, dt) {\n\
             let t = 0;\n\
             for i in 0..5 { t = helper(t); }\n\
         }\n",
    )]);
    let app = app_in(dir.path());
    let busy = spawn(&app, "Busy");
    let host = app.engine.script_host().unwrap();
    host.attach(balaur_core::node_id_of(busy), "busy.rn")
        .unwrap();

    assert!(host.function_costs().is_empty(), "nothing while it is off");
    host.set_profiling(true);
    host.update(1.0 / 60.0);
    host.update(1.0 / 60.0);
    let costs = host.function_costs();
    let (_, calls, instructions) = costs
        .iter()
        .find(|(function, _, _)| function.ends_with("helper"))
        .unwrap_or_else(|| panic!("helper went unpriced; got {costs:?}"));
    assert_eq!(*calls, 10, "five a tick, two ticks");
    assert!(*instructions >= 10, "{costs:?}");
    assert!(
        costs
            .iter()
            .any(|(function, _, _)| function.ends_with("update")),
        "the caller is its own row: {costs:?}"
    );
    host.set_profiling(false);
}
