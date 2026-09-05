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

/// The smallest PE whose security directory can be read: a DOS header
/// pointing at a PE32+ optional header, and sixteen data directories.
fn pe_template() -> Vec<u8> {
    let mut pe = vec![0u8; 0xf0];
    pe[0..2].copy_from_slice(b"MZ");
    pe[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    pe[0x40..0x44].copy_from_slice(b"PE\0\0");
    pe[0x58..0x5a].copy_from_slice(&0x20bu16.to_le_bytes());
    pe[0xc4..0xc8].copy_from_slice(&16u32.to_le_bytes());
    pe
}

/// What signtool does to a file: pad to eight bytes, append the certificate
/// table, and record where it went in the security directory.
fn authenticode_sign(game: &[u8], certificate: &[u8]) -> Vec<u8> {
    let mut out = game.to_vec();
    while out.len() % 8 != 0 {
        out.push(0);
    }
    let offset = out.len() as u32;
    out.extend_from_slice(certificate);
    let size = out.len() as u32 - offset;
    out[0xe8..0xec].copy_from_slice(&offset.to_le_bytes());
    out[0xec..0xf0].copy_from_slice(&size.to_le_bytes());
    out
}

/// Signing has to happen after fusing, so the certificate table lands after
/// the trailer and hides it from a reader that only looks at the end.
#[test]
fn a_signed_windows_game_still_finds_its_pack() {
    let pack = b"BPAK\x01a signed game's pack";
    let game = standalone::build(&pe_template(), pack);
    let signed = authenticode_sign(&game, b"a certificate table");

    assert_eq!(standalone::extract(&game), Some(pack.as_slice()));
    assert_eq!(standalone::extract(&signed), Some(pack.as_slice()));
}

/// The trailer is padded away from the table by up to seven bytes, and every
/// one of those offsets has to be found.
#[test]
fn the_padding_before_a_certificate_table_is_searched() {
    for extra in 0..8 {
        let pack = vec![b'p'; 16 + extra];
        let game = standalone::build(&pe_template(), &pack);
        let signed = authenticode_sign(&game, b"cert");
        assert_eq!(
            standalone::extract(&signed),
            Some(pack.as_slice()),
            "a pack of {} bytes was lost to padding",
            pack.len()
        );
    }
}

/// A signed binary that carries no pack is the signed CLI, and booting it as
/// a game would be the same bug the other way round.
#[test]
fn a_signed_binary_with_no_pack_is_not_a_game() {
    let signed = authenticode_sign(&pe_template(), b"a certificate table");
    assert_eq!(standalone::extract(&signed), None);
}

/// A directory pointing anywhere but the end of the file is not a table
/// appended after a pack, so nothing before it is a trailer.
#[test]
fn a_security_directory_that_does_not_end_the_file_is_ignored() {
    let game = standalone::build(&pe_template(), b"pack");
    let mut lying = authenticode_sign(&game, b"cert");
    lying.extend_from_slice(b"trailing bytes after the table");
    assert_eq!(standalone::extract(&lying), None);
}
