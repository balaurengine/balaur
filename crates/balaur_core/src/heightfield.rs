//! Terrain heights as a project resource: the `heightfield` asset type.
//!
//! Separate from `mesh` because a height grid is not geometry — it is a
//! regular lattice of one number per cell, which physics turns into a surface
//! far more cheaply than it could a triangle mesh.
//!
//! Lives in core rather than the render crate for the same reason `mesh` does:
//! physics reads it and does not depend on rendering.

use anyhow::{Result, anyhow, bail};

use crate::App;

pub const HEIGHTFIELD_ASSET_TYPE: &str = "heightfield";

/// A grid of heights, row-major.
///
/// The scale is deliberately not here: the same terrain at two sizes is one
/// resource used twice, so the collider that places it owns its extent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeightfieldData {
    pub rows: usize,
    pub columns: usize,
    /// `rows * columns` values, row-major.
    pub heights: Vec<f32>,
    /// Cells with no ground, as `[row, column]` of the cell's first corner.
    pub holes: Vec<[usize; 2]>,
}

impl HeightfieldData {
    /// The lowest and highest point, for sizing a bounding box.
    /// `None` when the grid is empty.
    #[must_use]
    pub fn range(&self) -> Option<(f32, f32)> {
        let first = *self.heights.first()?;
        Some(
            self.heights
                .iter()
                .fold((first, first), |(lo, hi), h| (lo.min(*h), hi.max(*h))),
        )
    }
}

/// What a definition table holds, for the generated reference.
const HEIGHTFIELD_ASSET_DOC: &str = r#"A grid of heights for terrain: `rows` by `columns` samples in `heights`, row-major, one value per grid point. `holes` lists cells with no ground, each as the `[row, column]` of its first corner; a 2D collider reads the heights as one line, and a hole there removes the segment starting at that point.

```toml
[[assets]]
id = "valley"
type = "heightfield"
rows = 3
columns = 3
heights = [0, 0, 0, 0, -1, 0, 0, 0, 0]
holes = [[1, 1]]
```"#;

/// Register `heightfield` so a scene, a component or a script can all name
/// terrain the same way:
///
/// ```toml
/// [[assets]]
/// id = "valley"
/// type = "heightfield"
/// rows = 3
/// columns = 3
/// heights = [0, 0, 0, 0, -1, 0, 0, 0, 0]
/// ```
pub(crate) fn register_heightfield_asset(app: &mut App) {
    app.register_asset_type(
        HEIGHTFIELD_ASSET_TYPE,
        "heightfields",
        HEIGHTFIELD_ASSET_DOC,
        |value| Ok(std::rc::Rc::new(parse_definition(value)?) as std::rc::Rc<dyn std::any::Any>),
    );
}

/// A `heightfield` definition. The count is checked here because the physics
/// backend asserts on a mismatched grid, and a panic is a poor way to learn
/// that a row is missing.
fn parse_definition(value: &toml::Value) -> Result<HeightfieldData> {
    let count = |key: &str| -> Result<usize> {
        let n = value
            .get(key)
            .and_then(toml::Value::as_integer)
            .ok_or_else(|| anyhow!("a heightfield needs an integer `{key}`"))?;
        usize::try_from(n).map_err(|_| anyhow!("a heightfield's `{key}` cannot be {n}"))
    };
    let rows = count("rows")?;
    let columns = count("columns")?;
    if rows < 2 || columns < 2 {
        bail!("a heightfield needs at least 2 rows and 2 columns, not {rows}x{columns}");
    }
    let heights: Vec<f32> = value
        .get("heights")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("a heightfield needs a `heights` list"))?
        .iter()
        .map(|h| {
            crate::components::as_f64(h)
                .map(|v| v as f32)
                .ok_or_else(|| anyhow!("a heightfield's `heights` must all be numbers"))
        })
        .collect::<Result<_>>()?;
    // Checked, not multiplied: the grid is read off a file and the product of
    // two numbers it chose need not fit.
    let wanted = rows
        .checked_mul(columns)
        .ok_or_else(|| anyhow!("a heightfield of {rows}x{columns} is larger than memory"))?;
    if heights.len() != wanted {
        bail!(
            "a heightfield of {rows}x{columns} needs {wanted} heights, but {} were given",
            heights.len()
        );
    }
    Ok(HeightfieldData {
        rows,
        columns,
        heights,
        holes: parse_holes(value, rows, columns)?,
    })
}

/// The `holes` list, each cell inside the grid: a grid of `rows` points has
/// `rows - 1` cells down it.
fn parse_holes(value: &toml::Value, rows: usize, columns: usize) -> Result<Vec<[usize; 2]>> {
    let Some(list) = value.get("holes") else {
        return Ok(Vec::new());
    };
    let list = list
        .as_array()
        .ok_or_else(|| anyhow!("a heightfield's `holes` must be a list of [row, column]"))?;
    list.iter()
        .map(|cell| {
            let at = |i: usize| {
                cell.as_array()
                    .and_then(|pair| pair.get(i))
                    .and_then(toml::Value::as_integer)
                    .and_then(|n| usize::try_from(n).ok())
            };
            match (at(0), at(1)) {
                (Some(row), Some(column)) if row + 1 < rows && column + 1 < columns => {
                    Ok([row, column])
                }
                _ => bail!(
                    "a heightfield of {rows}x{columns} has cells [0, 0] to [{}, {}], not {cell}",
                    rows - 2,
                    columns - 2
                ),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(source: &str) -> toml::Value {
        toml::from_str(source).expect("the test's own TOML parses")
    }

    #[test]
    fn a_grid_parses_row_major() {
        let data = parse_definition(&value(
            "rows = 2\ncolumns = 3\nheights = [0, 1, 2, 3, 4, 5]",
        ))
        .unwrap();
        assert_eq!((data.rows, data.columns), (2, 3));
        assert_eq!(data.heights.len(), 6);
    }

    /// parry's `Array2::new` asserts on this, and a panic is a poor way to
    /// learn that a row is short.
    #[test]
    fn a_grid_whose_count_does_not_match_its_shape_is_refused() {
        let err = parse_definition(&value("rows = 2\ncolumns = 3\nheights = [0, 1, 2]"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("needs 6 heights"), "{err}");
        assert!(err.contains("3 were given"), "{err}");
    }

    #[test]
    fn a_hole_names_a_cell_inside_the_grid() {
        let data = parse_definition(&value(
            "rows = 3\ncolumns = 3\nheights = [0, 0, 0, 0, 0, 0, 0, 0, 0]\nholes = [[1, 0]]",
        ))
        .unwrap();
        assert_eq!(data.holes, [[1, 0]]);
        let err = parse_definition(&value(
            "rows = 3\ncolumns = 3\nheights = [0, 0, 0, 0, 0, 0, 0, 0, 0]\nholes = [[2, 0]]",
        ))
        .unwrap_err()
        .to_string();
        assert!(err.contains("[1, 1]"), "{err}");
    }

    #[test]
    fn a_grid_thinner_than_one_cell_is_refused() {
        let err = parse_definition(&value("rows = 1\ncolumns = 3\nheights = [0, 1, 2]"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("at least 2 rows"), "{err}");
    }

    #[test]
    fn the_range_covers_every_height() {
        let data = parse_definition(&value(
            "rows = 2\ncolumns = 2\nheights = [-3.5, 0, 2, 7.25]",
        ))
        .unwrap();
        let (lo, hi) = data.range().expect("a populated grid has a range");
        assert!((lo + 3.5).abs() < 1e-6 && (hi - 7.25).abs() < 1e-6);
    }

    #[test]
    fn a_definition_missing_its_shape_says_so() {
        let err = parse_definition(&value("heights = [0, 1]"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("integer `rows`"), "{err}");
    }
}
