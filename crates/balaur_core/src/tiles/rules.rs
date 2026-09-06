//! Autotiling: which tile a cell gets, from what its neighbours are.
//!
//! One ordered table, first match wins. A rule is a pattern over the values
//! painted around a cell, an output tile, the turns the pattern may be
//! matched under, and a chance. The bitmask terrains other engines use are
//! sugar over the same table, so there is one resolver and one thing to
//! debug — and it is a pure function, so a headless test can assert a corner.

use anyhow::{Result, anyhow, bail};

use crate::components::as_f64;

/// How a terrain's tiles are chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Rules written out, which is what everything compiles to.
    #[default]
    Rules,
    /// The four sides decide, as a 4-bit mask.
    Sides,
    /// The four corners decide.
    Corners,
    /// Sides and corners together, the 47-tile blob.
    CornersAndSides,
}

/// A painted value, and what it is called.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Terrain {
    pub name: String,
    pub value: u32,
    pub mode: Mode,
}

/// What a cell of a pattern demands of the value at that neighbour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Demand {
    /// Anything at all, including the edge of the map.
    Any,
    /// The rule's own terrain.
    Same,
    /// Anything but the rule's own terrain, the empty cell included.
    Other,
    /// Nothing painted here.
    Empty,
    /// Anything painted here.
    Filled,
    /// Exactly this value.
    Exactly(u32),
    /// Any value but these.
    Not(Vec<u32>),
}

impl Demand {
    /// Whether a neighbour's value satisfies the demand.
    #[must_use]
    pub fn holds(&self, terrain: u32, value: Option<u32>) -> bool {
        match self {
            Self::Any => true,
            Self::Same => value == Some(terrain),
            Self::Other => value != Some(terrain),
            Self::Empty => value.is_none(),
            Self::Filled => value.is_some(),
            Self::Exactly(wanted) => value == Some(*wanted),
            Self::Not(refused) => value.is_none_or(|value| !refused.contains(&value)),
        }
    }
}

/// What lies past the edge of the map, as far as a rule is concerned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Outside {
    /// Nothing, which is what makes a level's border its own shape.
    #[default]
    Empty,
    /// More of the same, which is what keeps a border from breaking.
    Same,
    /// A value of its own.
    Value(u32),
}

/// The turns a rule may be matched under. One rule with both covers eight
/// orientations, which is what makes a 47-tile sheet a dozen rules.
pub const ROTATE: u8 = 1;
pub const MIRROR: u8 = 2;

/// One row of the table: a pattern, and the tile a cell gets when it matches.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    /// The value this rule paints.
    pub terrain: u32,
    /// `size * size` demands, row-major from the top left.
    pub pattern: Vec<Demand>,
    /// The neighbourhood's edge, always odd.
    pub size: usize,
    /// Tiles and their weights; one entry is the plain case.
    pub tiles: Vec<(u32, u32)>,
    /// `ROTATE`, `MIRROR`, or both.
    pub transforms: u8,
    /// How often the rule takes a cell it matched, 0..=1.
    pub chance: f32,
    pub outside: Outside,
}

impl Rule {
    fn demand(&self, dx: i32, dy: i32) -> &Demand {
        let half = (self.size / 2) as i32;
        let index = (dy + half) as usize * self.size + (dx + half) as usize;
        &self.pattern[index]
    }
}

/// One of the eight ways a pattern may be laid over a cell, as the offset it
/// samples and the flags the tile it places is drawn with.
const TURNS: [(fn(i32, i32) -> (i32, i32), u8); 8] = [
    (|x, y| (x, y), 0),
    (|x, y| (-y, x), super::TRANSPOSE | super::FLIP_X),
    (|x, y| (-x, -y), super::FLIP_X | super::FLIP_Y),
    (|x, y| (y, -x), super::TRANSPOSE | super::FLIP_Y),
    (|x, y| (-x, y), super::FLIP_X),
    (|x, y| (x, -y), super::FLIP_Y),
    (|x, y| (y, x), super::TRANSPOSE),
    (
        |x, y| (-y, -x),
        super::TRANSPOSE | super::FLIP_X | super::FLIP_Y,
    ),
];

/// The tile a cell gets, and how it is turned.
///
/// `values` answers what is painted at a coordinate; `None` is unpainted, and
/// a coordinate outside the map is the rule's own `outside`. Nothing is
/// random: an alternate is picked by hashing the cell's own coordinate, so
/// the same map resolves the same way twice, on any machine.
#[must_use]
pub fn resolve(
    rules: &[Rule],
    values: &dyn Fn(i32, i32) -> Option<u32>,
    inside: &dyn Fn(i32, i32) -> bool,
    column: i32,
    row: i32,
    seed: u64,
) -> Option<(u32, u8)> {
    let value = values(column, row)?;
    for (index, rule) in rules.iter().enumerate() {
        if rule.terrain != value {
            continue;
        }
        for (turn, (offset, flags)) in TURNS.iter().enumerate() {
            if !allowed(rule.transforms, turn) {
                continue;
            }
            if !matches(rule, values, inside, column, row, *offset) {
                continue;
            }
            let roll = hash(seed, column, row, index as u64);
            if rule.chance < 1.0 && (roll % 1000) as f32 / 1000.0 >= rule.chance {
                continue;
            }
            let tile = pick(&rule.tiles, roll >> 10)?;
            return Some((tile, *flags));
        }
    }
    None
}

/// Which of the eight turns a rule's `transforms` allows.
fn allowed(transforms: u8, turn: usize) -> bool {
    match turn {
        0 => true,
        1..=3 => transforms & ROTATE != 0,
        _ => transforms & MIRROR != 0,
    }
}

fn matches(
    rule: &Rule,
    values: &dyn Fn(i32, i32) -> Option<u32>,
    inside: &dyn Fn(i32, i32) -> bool,
    column: i32,
    row: i32,
    offset: fn(i32, i32) -> (i32, i32),
) -> bool {
    let half = (rule.size / 2) as i32;
    for dy in -half..=half {
        for dx in -half..=half {
            let (sx, sy) = offset(dx, dy);
            let (x, y) = (column + sx, row + sy);
            let value = if inside(x, y) {
                values(x, y)
            } else {
                match rule.outside {
                    Outside::Empty => None,
                    Outside::Same => Some(rule.terrain),
                    Outside::Value(value) => Some(value),
                }
            };
            if !rule.demand(dx, dy).holds(rule.terrain, value) {
                return false;
            }
        }
    }
    true
}

/// One tile out of the weighted list, by a number already rolled.
fn pick(tiles: &[(u32, u32)], roll: u64) -> Option<u32> {
    let total: u64 = tiles
        .iter()
        .map(|(_, weight)| u64::from(*weight).max(1))
        .sum();
    if total == 0 {
        return tiles.first().map(|(tile, _)| *tile);
    }
    let mut at = roll % total;
    for (tile, weight) in tiles {
        let weight = u64::from(*weight).max(1);
        if at < weight {
            return Some(*tile);
        }
        at -= weight;
    }
    tiles.last().map(|(tile, _)| *tile)
}

/// A number from a cell and a rule, so variation is reproducible: the same
/// map resolves to the same tiles on every machine and after every reload.
fn hash(seed: u64, column: i32, row: i32, rule: u64) -> u64 {
    let mut value = seed ^ 0x9e37_79b9_7f4a_7c15;
    for part in [column as i64 as u64, row as i64 as u64, rule] {
        value ^= part.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = value.rotate_left(31).wrapping_mul(0x94d0_49bb_1331_11eb);
    }
    value ^ (value >> 29)
}

/// `[[terrains]]` on a tileset.
pub fn parse_terrains(value: &toml::Value) -> Result<Vec<Terrain>> {
    let Some(list) = value.get("terrains") else {
        return Ok(Vec::new());
    };
    let list = list
        .as_array()
        .ok_or_else(|| anyhow!("a tileset's `terrains` is a list of tables"))?;
    list.iter()
        .enumerate()
        .map(|(index, table)| {
            let name = table
                .get("name")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| anyhow!("terrain {index} needs a `name`"))?
                .to_string();
            let value = table
                .get("value")
                .and_then(toml::Value::as_integer)
                .map_or(index as u32 + 1, |value| value as u32);
            let mode = match table.get("mode").and_then(toml::Value::as_str) {
                None | Some("rules") => Mode::Rules,
                Some("sides") => Mode::Sides,
                Some("corners") => Mode::Corners,
                Some("corners_and_sides") => Mode::CornersAndSides,
                Some(other) => bail!("terrain '{name}': '{other}' is not a mode"),
            };
            Ok(Terrain { name, value, mode })
        })
        .collect()
}

/// `[[rules]]` on a tileset, and the templates that write them for you.
pub fn parse_rules(value: &toml::Value) -> Result<Vec<Rule>> {
    let terrains = parse_terrains(value)?;
    let mut out = Vec::new();
    for terrain in &terrains {
        if terrain.mode == Mode::Rules {
            continue;
        }
        let first = value
            .get("terrains")
            .and_then(toml::Value::as_array)
            .and_then(|list| list.iter().find(|table| named(table, &terrain.name)))
            .and_then(|table| table.get("first_tile"))
            .and_then(toml::Value::as_integer)
            .unwrap_or(0) as u32;
        out.extend(template(terrain.mode, terrain.value, first));
    }
    let Some(list) = value.get("rules") else {
        return Ok(out);
    };
    let list = list
        .as_array()
        .ok_or_else(|| anyhow!("a tileset's `rules` is a list of tables"))?;
    for (index, table) in list.iter().enumerate() {
        out.push(parse_rule(index, table, &terrains)?);
    }
    Ok(out)
}

fn named(table: &toml::Value, name: &str) -> bool {
    table.get("name").and_then(toml::Value::as_str) == Some(name)
}

fn parse_rule(index: usize, table: &toml::Value, terrains: &[Terrain]) -> Result<Rule> {
    let terrain = match table.get("terrain") {
        Some(toml::Value::String(name)) => terrains
            .iter()
            .find(|terrain| terrain.name == *name)
            .map(|terrain| terrain.value)
            .ok_or_else(|| anyhow!("rule {index}: no terrain is called '{name}'"))?,
        Some(toml::Value::Integer(value)) => *value as u32,
        _ => bail!("rule {index} needs a `terrain`, by name or by value"),
    };
    let rows = table
        .get("pattern")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| anyhow!("rule {index} needs a `pattern` of rows"))?;
    let size = rows.len();
    if size % 2 == 0 || size < 3 {
        bail!("rule {index}: a pattern is 3, 5 or 7 rows, not {size}");
    }
    let mut pattern = Vec::with_capacity(size * size);
    for row in rows {
        let row = row
            .as_str()
            .ok_or_else(|| anyhow!("rule {index}: a pattern row is a string"))?;
        let cells: Vec<char> = row.chars().collect();
        if cells.len() != size {
            bail!(
                "rule {index}: a {size}-row pattern needs {size} cells per row, not {}",
                cells.len()
            );
        }
        for cell in cells {
            pattern.push(demand(index, cell)?);
        }
    }
    let tiles = parse_tiles_of(index, table)?;
    let mut transforms = 0;
    for word in table
        .get("transforms")
        .and_then(toml::Value::as_array)
        .unwrap_or(&Vec::new())
    {
        match word.as_str() {
            Some("rotate") => transforms |= ROTATE,
            Some("mirror") => transforms |= MIRROR,
            other => bail!("rule {index}: '{other:?}' is not a transform"),
        }
    }
    Ok(Rule {
        terrain,
        pattern,
        size,
        tiles,
        transforms,
        chance: table.get("chance").and_then(as_f64).unwrap_or(1.0) as f32,
        outside: match table.get("outside") {
            None | Some(toml::Value::String(_)) => match table
                .get("outside")
                .and_then(toml::Value::as_str)
                .unwrap_or("empty")
            {
                "empty" => Outside::Empty,
                "same" => Outside::Same,
                other => bail!("rule {index}: '{other}' is not an outside"),
            },
            Some(toml::Value::Integer(value)) => Outside::Value(*value as u32),
            Some(_) => bail!("rule {index}: `outside` is \"empty\", \"same\" or a value"),
        },
    })
}

fn parse_tiles_of(index: usize, table: &toml::Value) -> Result<Vec<(u32, u32)>> {
    if let Some(tile) = table.get("tile").and_then(toml::Value::as_integer) {
        return Ok(vec![(tile as u32, 1)]);
    }
    let list = table
        .get("tiles")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| anyhow!("rule {index} needs a `tile`, or `tiles` to pick between"))?;
    list.iter()
        .map(|entry| match entry {
            toml::Value::Integer(tile) => Ok((*tile as u32, 1)),
            table => {
                let tile = table
                    .get("tile")
                    .and_then(toml::Value::as_integer)
                    .ok_or_else(|| anyhow!("rule {index}: an alternate needs a `tile`"))?;
                let weight = table
                    .get("weight")
                    .and_then(toml::Value::as_integer)
                    .unwrap_or(1);
                Ok((tile as u32, weight.max(1) as u32))
            }
        })
        .collect()
}

fn demand(index: usize, cell: char) -> Result<Demand> {
    Ok(match cell {
        '?' => Demand::Any,
        '#' => Demand::Same,
        '.' => Demand::Other,
        'x' => Demand::Empty,
        '*' => Demand::Filled,
        '0'..='9' => Demand::Exactly(cell as u32 - '0' as u32),
        other => bail!("rule {index}: '{other}' is not a pattern cell"),
    })
}

/// The rules a standard sheet layout is: the tiles are in a known order, so
/// the table can be written for you and the odd one fixed by hand.
///
/// `first` is the tile the layout starts at.
#[must_use]
pub fn template(mode: Mode, terrain: u32, first: u32) -> Vec<Rule> {
    match mode {
        Mode::Rules => Vec::new(),
        Mode::Sides => mask_rules(terrain, first, &SIDES),
        Mode::Corners => mask_rules(terrain, first, &CORNERS),
        Mode::CornersAndSides => blob_rules(terrain, first),
    }
}

/// North, east, south, west: the order a 16-tile sheet counts its bits in.
const SIDES: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
/// North-west, north-east, south-east, south-west.
const CORNERS: [(i32, i32); 4] = [(-1, -1), (1, -1), (1, 1), (-1, 1)];

/// Sixteen rules, one per combination of the four neighbours that count.
fn mask_rules(terrain: u32, first: u32, neighbours: &[(i32, i32); 4]) -> Vec<Rule> {
    (0..16u32)
        .map(|mask| {
            let mut pattern = vec![Demand::Any; 9];
            pattern[4] = Demand::Same;
            for (bit, (dx, dy)) in neighbours.iter().enumerate() {
                let index = ((dy + 1) * 3 + (dx + 1)) as usize;
                pattern[index] = if mask & (1 << bit) != 0 {
                    Demand::Same
                } else {
                    Demand::Other
                };
            }
            Rule {
                terrain,
                pattern,
                size: 3,
                tiles: vec![(first + mask, 1)],
                transforms: 0,
                chance: 1.0,
                outside: Outside::Empty,
            }
        })
        .collect()
}

/// The 47-tile blob: every neighbourhood whose corners are backed by both of
/// their sides, in ascending mask order, which is how the sheets are laid out.
fn blob_rules(terrain: u32, first: u32) -> Vec<Rule> {
    let mut out = Vec::new();
    let mut index = 0;
    for mask in 0..256u32 {
        if !canonical(mask) {
            continue;
        }
        let mut pattern = vec![Demand::Any; 9];
        pattern[4] = Demand::Same;
        for (bit, (dx, dy)) in NEIGHBOURS.iter().enumerate() {
            let at = ((dy + 1) * 3 + (dx + 1)) as usize;
            let set = mask & (1 << bit) != 0;
            let corner = dx.abs() == 1 && dy.abs() == 1;
            pattern[at] = if set {
                Demand::Same
            } else if corner {
                // A corner nobody can see is not a demand: the sides decide.
                Demand::Any
            } else {
                Demand::Other
            };
        }
        out.push(Rule {
            terrain,
            pattern,
            size: 3,
            tiles: vec![(first + index, 1)],
            transforms: 0,
            chance: 1.0,
            outside: Outside::Empty,
        });
        index += 1;
    }
    out
}

/// The eight neighbours, sides first, in the order the blob counts its bits.
const NEIGHBOURS: [(i32, i32); 8] = [
    (0, -1),
    (1, 0),
    (0, 1),
    (-1, 0),
    (-1, -1),
    (1, -1),
    (1, 1),
    (-1, 1),
];

/// Whether a mask is one of the 47: a corner counts only where both of the
/// sides beside it are set.
fn canonical(mask: u32) -> bool {
    let side = |bit: u32| mask & (1 << bit) != 0;
    let corner = |bit: u32| mask & (1 << bit) != 0;
    let pairs = [(4, 3, 0), (5, 0, 1), (6, 1, 2), (7, 2, 3)];
    pairs
        .iter()
        .all(|(corner_bit, a, b)| !corner(*corner_bit) || (side(*a) && side(*b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A map of painted values, as a resolver reads one.
    fn painted<'a>(rows: &'a [&'a [i32]]) -> impl Fn(i32, i32) -> Option<u32> + 'a {
        move |x: i32, y: i32| {
            let row = usize::try_from(y).ok()?;
            let column = usize::try_from(x).ok()?;
            match rows.get(row)?.get(column)? {
                value if *value >= 0 => Some(*value as u32),
                _ => None,
            }
        }
    }

    fn inside<'a>(rows: &'a [&'a [i32]]) -> impl Fn(i32, i32) -> bool + 'a {
        move |x: i32, y: i32| {
            x >= 0 && y >= 0 && (y as usize) < rows.len() && (x as usize) < rows[y as usize].len()
        }
    }

    #[test]
    fn the_sides_template_names_a_tile_per_neighbourhood() {
        let rules = template(Mode::Sides, 1, 0);
        assert_eq!(rules.len(), 16);
        let rows: &[&[i32]] = &[&[-1, 1, -1], &[1, 1, 1], &[-1, 1, -1]];
        let (tile, _) = resolve(&rules, &painted(rows), &inside(rows), 1, 1, 7)
            .expect("the middle cell has all four sides");
        assert_eq!(tile, 15, "north, east, south and west are all the terrain");
        let (corner, _) = resolve(&rules, &painted(rows), &inside(rows), 1, 0, 7)
            .expect("the top cell is painted too");
        assert_eq!(corner, 4, "only its south side is the terrain");
    }

    #[test]
    fn the_blob_template_is_forty_seven_tiles() {
        assert_eq!(template(Mode::CornersAndSides, 1, 0).len(), 47);
    }

    #[test]
    fn what_lies_past_the_edge_is_the_rule_s_own_business() {
        let rows: &[&[i32]] = &[&[1]];
        let mut rules = template(Mode::Sides, 1, 0);
        let (alone, _) = resolve(&rules, &painted(rows), &inside(rows), 0, 0, 1).unwrap();
        assert_eq!(alone, 0, "with nothing outside, the cell stands alone");
        for rule in &mut rules {
            rule.outside = Outside::Same;
        }
        let (surrounded, _) = resolve(&rules, &painted(rows), &inside(rows), 0, 0, 1).unwrap();
        assert_eq!(surrounded, 15, "with more of the same, it is an interior");
    }

    #[test]
    fn variation_is_hashed_so_two_runs_agree() {
        let rules = vec![Rule {
            terrain: 1,
            pattern: vec![Demand::Any; 9],
            size: 3,
            tiles: vec![(1, 1), (2, 1), (3, 1)],
            transforms: 0,
            chance: 1.0,
            outside: Outside::Empty,
        }];
        let rows: &[&[i32]] = &[&[1, 1, 1], &[1, 1, 1]];
        let once: Vec<_> = (0..3)
            .map(|x| {
                resolve(&rules, &painted(rows), &inside(rows), x, 0, 99)
                    .unwrap()
                    .0
            })
            .collect();
        let twice: Vec<_> = (0..3)
            .map(|x| {
                resolve(&rules, &painted(rows), &inside(rows), x, 0, 99)
                    .unwrap()
                    .0
            })
            .collect();
        assert_eq!(once, twice, "the same cells resolve the same way");
        let other: Vec<_> = (0..3)
            .map(|x| {
                resolve(&rules, &painted(rows), &inside(rows), x, 0, 4)
                    .unwrap()
                    .0
            })
            .collect();
        assert!(
            once != other || once.iter().all(|tile| *tile == once[0]),
            "a different seed may lay them out differently"
        );
    }

    #[test]
    fn a_turned_rule_covers_the_orientations_it_was_not_written_for() {
        let rule = Rule {
            terrain: 1,
            pattern: vec![
                Demand::Any,
                Demand::Other,
                Demand::Any,
                Demand::Any,
                Demand::Same,
                Demand::Any,
                Demand::Any,
                Demand::Any,
                Demand::Any,
            ],
            size: 3,
            tiles: vec![(5, 1)],
            transforms: ROTATE,
            chance: 1.0,
            outside: Outside::Same,
        };
        let rows: &[&[i32]] = &[&[1, 1, 1], &[1, 1, -1], &[1, 1, 1]];
        let (tile, flags) = resolve(&[rule], &painted(rows), &inside(rows), 1, 1, 3)
            .expect("the empty cell is east, not north");
        assert_eq!(tile, 5);
        assert_ne!(flags, 0, "so the tile is drawn turned");
    }
}
