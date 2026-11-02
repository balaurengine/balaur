//! A pack made straight from a project's files, for a test of a packed run
//! that needs no script compiled: building a pack is `balaur_project`'s.

use std::collections::BTreeMap;
use std::path::Path;

use balaur_core::Pack;

/// Every file under `root`: `project.toml` the manifest, a `.toml` a
/// document, anything else an asset.
pub(crate) fn of_files(root: &Path) -> Pack {
    let mut files = BTreeMap::new();
    gather(root, root, &mut files);
    let mut pack = Pack::default();
    for (rel, bytes) in files {
        if rel == "project.toml" {
            pack.manifest = String::from_utf8(bytes).unwrap();
        } else if Path::new(&rel).extension().is_some_and(|ext| ext == "toml") {
            pack.scenes.insert(rel, String::from_utf8(bytes).unwrap());
        } else {
            pack.assets.insert(rel, bytes);
        }
    }
    pack
}

fn gather(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            gather(root, &path, out);
        } else {
            let rel = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(rel, std::fs::read(&path).unwrap());
        }
    }
}
