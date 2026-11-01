//! What a script no longer writes to the engine, refused by name with the form
//! that replaced it. The engine's own modules are out of this crate's sight, so
//! the names are spelled here.

/// Why `name`, which the engine no longer has, is gone, and what to write.
pub(crate) fn function(name: &str) -> Option<String> {
    let (module, function) = name.split_once("::")?;
    Some(match (module, function) {
        ("export" | "import" | "release" | "forge" | "picture" | "browser" | "apple", "listen") => {
            format!(
                "`{name}` is gone: `events::listen(node, \"{module}_event\")`, heard as `on_{module}_event`"
            )
        }
        ("browser", "stop_listening") => {
            "`browser::stop_listening` is gone: `events::stop_listening(node, \"browser_event\")`"
                .to_string()
        }
        _ => return None,
    })
}

/// Whether `name` is a call whose one reply is what `task::wait` answers, with
/// no node to tell.
pub(crate) fn answers_by_await(name: &str) -> bool {
    let Some((module, function)) = name.split_once("::") else {
        return false;
    };
    match module {
        "http" => function == "request",
        "gamend" => matches!(function, "login" | "register" | "rest"),
        "platform" => matches!(
            function,
            "sign_in"
                | "set_presence"
                | "unlock"
                | "progress"
                | "submit_score"
                | "scores"
                | "cloud_read"
                | "cloud_write"
        ),
        "apple" => matches!(
            function,
            "identity"
                | "sign_in"
                | "credential_state"
                | "show_dashboard"
                | "request_notifications"
                | "notify"
                | "products"
                | "purchase"
                | "entitlements"
                | "restore_purchases"
                | "finish_purchase"
        ),
        _ => false,
    }
}
