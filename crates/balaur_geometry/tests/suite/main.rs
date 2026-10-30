//! Every integration test this crate has, in one binary.

mod csg;
mod path;
mod polygon_mesh;
mod primitive;
mod robustness;
mod skeleton;

use balaur_core::{App, AppConfig};

/// An app at `root` with this crate's assets and components registered after
/// the kernel's: what an engine registers, less the crates this one does not
/// know.
pub(crate) fn app_at(root: impl Into<std::path::PathBuf>) -> App {
    let mut app = App::new(AppConfig::bare(root)).unwrap();
    balaur_geometry::mesh::register_mesh_asset(&mut app);
    balaur_geometry::path::register_path_assets(&mut app);
    balaur_geometry::heightfield::register_heightfield_asset(&mut app);
    balaur_geometry::voxels::register_voxels_asset(&mut app);
    balaur_geometry::skeleton::register_bone2d_component(&mut app);
    balaur_geometry::skeleton::register_bone3d_component(&mut app);
    app
}
