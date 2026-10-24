//! Porting a project on disk: drafts beside the sources, a person's own
//! `.bs` kept unless replacing it is asked for.

use balaur_port::Api;
use balaur_port::files::{Dest, port_tree};

#[test]
fn a_port_writes_beside_each_file_and_keeps_what_a_person_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let scripts = dir.path().join("scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(scripts.join("a.rn"), "pub fn twice(x) { x * 2 }\n").unwrap();
    std::fs::write(scripts.join("b.rn"), "pub fn one() { 1 }\n").unwrap();
    std::fs::write(scripts.join("b.bs"), "// mine\n").unwrap();
    let paths = [dir.path().to_path_buf()];

    let done = port_tree(&paths, Api::default(), &Dest::Beside { force: false }).unwrap();
    assert_eq!(done.written, [scripts.join("a.bs")]);
    assert_eq!(done.kept, [scripts.join("b.bs")]);
    assert!(
        std::fs::read_to_string(scripts.join("a.bs"))
            .unwrap()
            .contains("fn twice")
    );
    assert_eq!(
        std::fs::read_to_string(scripts.join("b.bs")).unwrap(),
        "// mine\n"
    );

    let done = port_tree(&paths, Api::default(), &Dest::Beside { force: true }).unwrap();
    assert_eq!(done.written.len(), 2, "forced, both are written");
    assert!(
        std::fs::read_to_string(scripts.join("b.bs"))
            .unwrap()
            .contains("fn one")
    );

    let out = dir.path().join("out");
    port_tree(&paths, Api::default(), &Dest::Under(&out)).unwrap();
    assert!(out.join("SUMMARY.md").is_file());
    assert!(
        out.join("a.port.md").is_file(),
        "paths are kept from the files' common root"
    );
}
