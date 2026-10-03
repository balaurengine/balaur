//! The whole-grid edits on a 3D voxel collider, the twins of `physics2d`'s:
//! resizing its cells, cropping it, and joining two grids' seams.

use balaur_physics::PhysicsState3d;
use balaur_physics::rapier3d::math::IVector;
use balaur_script::Value;

use crate::boot::{self, Booted};

fn collider<R>(
    b: &Booted,
    path: &str,
    f: impl FnOnce(&balaur_physics::rapier3d::prelude::Collider) -> R,
) -> R {
    let node = b.node(path);
    let state = b.app.engine.resource::<PhysicsState3d>();
    let state = state.borrow();
    f(&state.world.colliders[state.colliders[&node][0]])
}

/// A row of three cells beside a single cell, the same row alone, and the
/// row again to crop.
const GRIDS: &str = r##"[[assets]]
id = "row"
type = "voxels"
size = [1.0, 1.0, 1.0]
cells = [[0, 0, 0], [1, 0, 0], [2, 0, 0]]

[[assets]]
id = "one"
type = "voxels"
size = [1.0, 1.0, 1.0]
cells = [[0, 0, 0]]

[[nodes]]
id = "n_world"
name = "World"
script = { source = "scripts/s.rn" }

[[nodes]]
id = "n_a"
name = "A"
parent = "n_world"
collider3d = { kind = "voxels", voxels = "#row" }

[[nodes]]
id = "n_b"
name = "B"
parent = "n_world"
collider3d = { kind = "voxels", voxels = "#one" }
transform = { position = [3.0, 0.0, 0.0] }

[[nodes]]
id = "n_alone"
name = "Alone"
parent = "n_world"
collider3d = { kind = "voxels", voxels = "#row" }
transform = { position = [0.0, 5.0, 0.0] }

[[nodes]]
id = "n_cropped"
name = "Cropped"
parent = "n_world"
collider3d = { kind = "voxels", voxels = "#row" }
transform = { position = [0.0, 10.0, 0.0] }
"##;

#[test]
fn a_3d_voxel_grid_resizes_crops_and_joins_its_neighbour() {
    let mut b = boot::project(
        GRIDS,
        r#"pub fn init(this) {
    let a = this.node.get_node("A").collider3d;
    a.combine_voxels(this.node.get_node("B"));
    let cropped = this.node.get_node("Cropped").collider3d;
    cropped.crop_voxels(0, 0, 0, 1, 0, 0);
    let alone = this.node.get_node("Alone").collider3d;
    alone.set_voxel_size(0.5, 0.5, 0.25);
}

pub fn probe(this) {
    this.node.get_node("Alone").collider3d.voxel_size()
}
"#,
    );
    b.tick(1);
    let errors = boot::errors();
    assert!(errors.is_empty(), "the scene logged errors: {errors:#?}");
    let state_at = |path: &str, x: i32| {
        collider(&b, path, |c| {
            c.shape()
                .as_voxels()
                .unwrap()
                .voxel_state(IVector::new(x, 0, 0))
        })
    };
    assert_ne!(
        state_at("World/A", 2),
        state_at("World/Alone", 2),
        "combining left the end cell unaware of its neighbour"
    );
    assert!(
        state_at("World/Cropped", 2)
            .is_none_or(balaur_physics::rapier3d::prelude::VoxelState::is_empty),
        "crop kept a cell outside the range"
    );
    assert!(state_at("World/Cropped", 0).is_some_and(|s| !s.is_empty()));
    assert_eq!(
        b.call(b.node("World"), "probe"),
        Value::Vec3([0.5, 0.5, 0.25])
    );
}
