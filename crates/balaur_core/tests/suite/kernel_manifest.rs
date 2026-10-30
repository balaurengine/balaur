//! The kernel's dependencies (PLAN-core-split §2): it uses none of the crates
//! around it, and none of the geometry, image, model or mesh crates they
//! took with them, so the split cannot grow back one `use` at a time.

/// Crates the kernel must not depend on: the five around it, and what each
/// of them brought.
const KEPT_OUT: &[&str] = &[
    "balaur_geometry",
    "balaur_asset",
    "balaur_net",
    "balaur_project",
    "balaur_debug",
    "balaur",
    "gltf",
    "image",
    "i_overlay",
    "i_triangle",
    "meshopt-rs",
    "resvg",
    "usvg",
    "tiny-skia",
];

#[test]
fn the_kernel_depends_on_none_of_the_crates_around_it() {
    let manifest: toml::Table = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/Cargo.toml"
    ))
    .unwrap()
    .parse()
    .unwrap();
    let mut named = Vec::new();
    collect(&manifest, &mut named);
    let found: Vec<&String> = named
        .iter()
        .filter(|name| KEPT_OUT.contains(&name.as_str()))
        .collect();
    assert!(
        found.is_empty(),
        "balaur_core depends on {found:?}; those belong to the crates around it"
    );
}

/// Every dependency the manifest names, in any section: plain, dev, build
/// and per target.
fn collect(table: &toml::Table, out: &mut Vec<String>) {
    for (key, value) in table {
        let Some(inner) = value.as_table() else {
            continue;
        };
        if key.ends_with("dependencies") {
            out.extend(inner.keys().cloned());
        } else {
            collect(inner, out);
        }
    }
}
