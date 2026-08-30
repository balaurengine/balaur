//! A pack carried inside the executable that runs it.

use balaur_core::standalone;

#[test]
fn a_standalone_binary_gives_the_pack_back() {
    let template = b"\x7fELF not really, but trailing bytes are ignored either way";
    let pack = b"BPAK\x01...contents...";
    let game = standalone::build(template, pack);
    assert_eq!(standalone::extract(&game), Some(pack.as_slice()));
    assert!(
        game.starts_with(template),
        "the template must still be intact, or the result does not run"
    );
}

#[test]
fn a_plain_binary_has_no_pack() {
    assert_eq!(standalone::extract(b"just a normal executable"), None);
    assert_eq!(standalone::extract(b""), None);
    assert_eq!(standalone::extract(b"BPAKSELF"), None);
}

#[test]
fn a_lying_trailer_is_refused_rather_than_panicking() {
    let mut forged = b"tiny".to_vec();
    forged.extend_from_slice(&u64::MAX.to_le_bytes());
    forged.extend_from_slice(b"BPAKSELF");
    assert_eq!(standalone::extract(&forged), None);

    let mut nearly = b"tiny".to_vec();
    nearly.extend_from_slice(&99u64.to_le_bytes());
    nearly.extend_from_slice(b"BPAKSELF");
    assert_eq!(standalone::extract(&nearly), None);
}

#[test]
fn an_empty_pack_still_round_trips() {
    let game = standalone::build(b"template", b"");
    assert_eq!(standalone::extract(&game), Some(b"".as_slice()));
}

#[test]
fn a_standalone_file_is_read_back_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let template = dir.path().join("runtime");
    std::fs::write(&template, b"pretend engine").unwrap();
    let game = dir.path().join("game");
    let bytes = standalone::build(b"pretend engine", b"BPAK\x01payload");
    standalone::write_executable(&game, &bytes, &template).unwrap();

    assert_eq!(
        standalone::extract_from(&game).unwrap().as_deref(),
        Some(b"BPAK\x01payload".as_slice())
    );
    assert_eq!(standalone::extract_from(&template).unwrap(), None);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&game).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "an exported game must be executable");
    }
}
