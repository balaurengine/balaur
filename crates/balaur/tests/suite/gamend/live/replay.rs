//! A session recorded against a live server replays with none.

use crate::gamend::replay::{project, record, replayed};
use crate::gamend::{delete_account, sign_in, tag};

#[test]
fn a_recorded_session_with_gamend_traffic_replays_with_no_server() {
    let Some(server) = crate::gamend::server() else {
        return;
    };
    let tag = tag();
    let device = format!("balaur-replay-{tag}");
    let dir = tempfile::tempdir().unwrap();
    project(dir.path(), &server, &device, &tag);
    let (recorded, heard) =
        std::thread::scope(|scope| scope.spawn(|| record(dir.path())).join().unwrap());
    if let Some((_, token)) = sign_in(&server, &device) {
        assert_eq!(
            delete_account(&server, &token, ""),
            200,
            "the probe's account stayed"
        );
    }
    assert_eq!(
        heard, "login 200 201 open ok error 200 true",
        "the recording itself"
    );
    // A replay sends nothing: the server is pointed nowhere, and still the
    // probe hears what it heard.
    let elsewhere = tempfile::tempdir().unwrap();
    project(elsewhere.path(), "http://127.0.0.1:9", &device, &tag);
    let again = replayed(elsewhere.path().to_path_buf(), recorded);
    assert_eq!(again, heard, "the replay heard something else");
}
