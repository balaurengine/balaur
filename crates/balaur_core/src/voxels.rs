//! A voxel grid as a project resource: the `voxels` asset type.
//!
//! Separate from `mesh` for the reason `heightfield` is: a voxel grid is not
//! geometry but a set of filled cells on a lattice, and physics turns one into
//! a collider far more cheaply than it could a triangle soup — and, unlike a
//! mesh, a game may dig a hole in one at run time.
//!
//! Lives in core rather than in physics because an asset type belongs to the
//! asset layer, and because a renderer will want to read the same grid.

use anyhow::{anyhow, bail, Result};

use crate::App;

pub const VOXELS_ASSET_TYPE: &str = "voxels";

/// The filled cells of a grid, and how big one cell is.
///
/// Coordinates are signed and unbounded: a voxel world has no origin corner,
/// and a hole dug at the edge must not have to grow an array.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VoxelsData {
    /// The size of one cell, in world units.
    pub size: [f32; 3],
    /// The filled cells, as integer grid coordinates.
    pub cells: Vec<[i32; 3]>,
}

const VOXELS_ASSET_DOC: &str = r#"A voxel grid for a collider: `size` is one cell in world units, `cells` the
filled coordinates. Coordinates are signed, so a grid has no origin corner,
and `physics3d.set_voxel` may add or remove a cell at run time.

```toml
[[assets]]
id = "pillar"
type = "voxels"
size = [1.0, 1.0, 1.0]
cells = [[0, 0, 0], [0, 1, 0], [0, 2, 0]]
```"#;

pub(crate) fn register_voxels_asset(app: &mut App) {
    app.register_asset_type(VOXELS_ASSET_TYPE, "terrain", VOXELS_ASSET_DOC, |value| {
        Ok(std::rc::Rc::new(parse_definition(value)?) as std::rc::Rc<dyn std::any::Any>)
    });
}

fn parse_definition(value: &toml::Value) -> Result<VoxelsData> {
    let size = value
        .get("size")
        .and_then(toml::Value::as_array)
        .map_or([1.0; 3], |a| {
            let at = |i: usize| a.get(i).and_then(crate::components::as_f64).unwrap_or(1.0) as f32;
            [at(0), at(1), at(2)]
        });
    if size.iter().any(|s| *s <= 0.0) {
        bail!("a voxel grid's `size` must be positive in every axis, not {size:?}");
    }
    let cells = value
        .get("cells")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| anyhow!("a voxel grid needs a `cells` list of [x, y, z] coordinates"))?
        .iter()
        .map(|cell| {
            let cell = cell
                .as_array()
                .ok_or_else(|| anyhow!("every entry of `cells` is an [x, y, z] triple"))?;
            let at = |i: usize| -> Result<i32> {
                let value = cell
                    .get(i)
                    .and_then(toml::Value::as_integer)
                    .ok_or_else(|| anyhow!("a voxel coordinate must be a whole number"))?;
                i32::try_from(value).map_err(|_| anyhow!("the voxel coordinate {value} is too far"))
            };
            Ok([at(0)?, at(1)?, at(2)?])
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(VoxelsData { size, cells })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grid_parses_its_cells() {
        let value: toml::Value =
            toml::from_str("size = [0.5, 0.5, 0.5]\ncells = [[0, 0, 0], [1, 0, -2]]").unwrap();
        let data = parse_definition(&value).unwrap();
        // Bits, because what is asserted is that the literal survived parsing.
        assert_eq!(data.size.map(f32::to_bits), [0.5f32.to_bits(); 3]);
        assert_eq!(data.cells, vec![[0, 0, 0], [1, 0, -2]]);
    }

    #[test]
    fn a_grid_without_cells_is_refused() {
        let value: toml::Value = toml::from_str("size = [1, 1, 1]").unwrap();
        assert!(parse_definition(&value).is_err());
    }
}
