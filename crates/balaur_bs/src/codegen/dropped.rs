//! What a script no longer writes to the engine, refused by name with the form
//! that replaced it. The engine's own modules are out of this crate's sight, so
//! the names are spelled here.

/// Why `name`, which the engine no longer has, is gone, and what to write.
use crate::vocabulary::words;

const RENDER: &str = "render";
pub(crate) fn function(name: &str) -> Option<String> {
    let (module, function) = name.split_once("::")?;
    Some(match (module, function) {
        (words::droppable::EXPORT | words::droppable::IMPORT | words::droppable::RELEASE | words::droppable::FORGE | words::droppable::PICTURE | words::droppable::BROWSER | words::droppable::APPLE, words::droppable::LISTEN) => {
            format!(
                "`{name}` is gone: `events::listen(node, \"{module}_event\")`, heard as `on_{module}_event`"
            )
        }
        (words::droppable::BROWSER, words::droppable::STOP_LISTENING) => {
            "`browser::stop_listening` is gone: `events::stop_listening(node, \"browser_event\")`"
                .to_string()
        }
        (RENDER, words::droppable::SET_CAMERA) => {
            "`render::set_camera` is gone: a `camera3d` node looks from its position at its `look_at`"
                .to_string()
        }
        (RENDER, words::droppable::SET_CAMERA_2D) => {
            "`render::set_camera_2d` is gone: a `camera2d` node looks from its position, at its `pixels_per_unit`"
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
        words::droppable::HTTP => function == words::droppable::REQUEST,
        words::droppable::GAMEND => matches!(
            function,
            words::droppable::LOGIN | words::droppable::REGISTER | words::droppable::REST
        ),
        words::droppable::PLATFORM => matches!(
            function,
            words::droppable::SIGN_IN
                | words::droppable::SET_PRESENCE
                | words::droppable::UNLOCK
                | words::droppable::PROGRESS
                | words::droppable::SUBMIT_SCORE
                | words::droppable::SCORES
                | words::droppable::CLOUD_READ
                | words::droppable::CLOUD_WRITE
        ),
        words::droppable::APPLE => matches!(
            function,
            words::droppable::IDENTITY
                | words::droppable::SIGN_IN
                | words::droppable::CREDENTIAL_STATE
                | words::droppable::SHOW_DASHBOARD
                | words::droppable::REQUEST_NOTIFICATIONS
                | words::droppable::NOTIFY
                | words::droppable::PRODUCTS
                | words::droppable::PURCHASE
                | words::droppable::ENTITLEMENTS
                | words::droppable::RESTORE_PURCHASES
                | words::droppable::FINISH_PURCHASE
        ),
        _ => false,
    }
}
