//! The parsers that read bytes somebody else wrote, against bytes nobody
//! wrote: each answers a corrupted input with an error rather than a panic.

#[path = "../../../balaur_core/tests/support/corrupt.rs"]
mod corrupt;
use corrupt::corruptions;

// An `.obj` is text with counts and indices in it, which is where a reader
// that trusts the file indexes past the end of its own vertex list.
#[test]
fn a_corrupted_obj_is_an_error_and_never_a_panic() {
    let valid = b"# a quad\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\n\
                  vt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\n\
                  vn 0 0 1\n\
                  f 1/1/1 2/2/1 3/3/1\nf 1/1/1 3/3/1 4/4/1\n";
    assert!(
        balaur_geometry::mesh::parse_obj(valid, "q.obj").is_ok(),
        "the corpus itself must parse"
    );
    for (case, bytes) in corruptions(valid, 2000).into_iter().enumerate() {
        let read = std::panic::catch_unwind(|| balaur_geometry::mesh::parse_obj(&bytes, "q.obj"));
        assert!(
            read.is_ok(),
            "case {case}: parse_obj panicked on {} bytes",
            bytes.len()
        );
    }
}
