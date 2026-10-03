//! A `grid`'s tracks, areas and placements, read from the words a scene
//! writes into the types taffy lays out with.
//!
//! The words are CSS grid's with Balaur's spelling: `120` is design pixels,
//! `1fr`, `25%`, `auto`, `min_content`, `max_content`, `fit_content(120)`,
//! `minmax(80, 1fr)` and `repeat(3, 1fr)`, whose count may be `auto_fill` or
//! `auto_fit`. A placement is a line, `1 / 3`, `span 2` or an area's name.

use taffy::geometry::Line;
use taffy::style::{
    GridPlacement, GridTemplateArea, GridTemplateAreas, GridTemplateComponent,
    MaxTrackSizingFunction, MinTrackSizingFunction, RepetitionCount, TrackSizingFunction,
};

use crate::vocabulary::words::AUTO;
use crate::vocabulary::words::track as t;
use crate::widget::node::Layout;

/// Refuse a layout whose grid words do not read, saying which and why.
///
/// # Errors
/// The first key that does not read.
pub(crate) fn check(layout: &Layout) -> anyhow::Result<()> {
    use crate::vocabulary::keys as k;
    let named = |key: &str, why: String| anyhow::anyhow!("widget: `{key}`: {why}");
    template(&layout.grid_columns).map_err(|why| named(k::GRID_COLUMNS, why))?;
    template(&layout.grid_rows).map_err(|why| named(k::GRID_ROWS, why))?;
    auto_tracks(&layout.auto_columns).map_err(|why| named(k::AUTO_COLUMNS, why))?;
    auto_tracks(&layout.auto_rows).map_err(|why| named(k::AUTO_ROWS, why))?;
    areas(&layout.areas).map_err(|why| named(k::AREAS, why))?;
    placement(&layout.row).map_err(|why| named(k::ROW, why))?;
    placement(&layout.column).map_err(|why| named(k::COLUMN, why))?;
    Ok(())
}

/// The words split at spaces outside brackets, which is where one track ends.
fn words(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = None;
    for (at, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(from) = start.take() {
                out.push(&text[from..at]);
            }
        } else if start.is_none() {
            start = Some(at);
        }
    }
    if let Some(from) = start {
        out.push(&text[from..]);
    }
    out
}

/// What `name(...)` holds, split at its top-level commas; `None` for a word
/// that is not that call.
fn call<'a>(word: &'a str, name: &str) -> Option<Vec<&'a str>> {
    let inner = word.strip_prefix(name)?.trim_start().strip_prefix('(')?;
    let inner = inner.strip_suffix(')')?;
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut from = 0;
    for (at, c) in inner.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(inner[from..at].trim());
                from = at + 1;
            }
            _ => {}
        }
    }
    out.push(inner[from..].trim());
    Some(out)
}

fn number(word: &str) -> Result<f32, String> {
    word.parse::<f32>()
        .ok()
        .filter(|n| n.is_finite() && *n >= 0.0)
        .ok_or_else(|| format!("`{word}` is not a size"))
}

/// The low end of a `minmax`: a fixed size or a content size, never a share.
fn low(word: &str) -> Result<MinTrackSizingFunction, String> {
    Ok(match word {
        AUTO => MinTrackSizingFunction::auto(),
        t::MIN_CONTENT => MinTrackSizingFunction::min_content(),
        t::MAX_CONTENT => MinTrackSizingFunction::max_content(),
        _ => match word.strip_suffix(t::PERCENT) {
            Some(share) => MinTrackSizingFunction::percent(number(share)? / 100.0),
            None if word.ends_with(t::FR) => {
                return Err(format!("`{word}`: a share cannot be a minimum"));
            }
            None => MinTrackSizingFunction::length(number(word)?),
        },
    })
}

/// The high end of a `minmax`, which may also be a share or `fit_content`.
fn high(word: &str) -> Result<MaxTrackSizingFunction, String> {
    if let Some(args) = call(word, t::FIT_CONTENT) {
        let [limit] = args.as_slice() else {
            return Err(format!("`{word}` takes one size"));
        };
        return Ok(match limit.strip_suffix(t::PERCENT) {
            Some(share) => MaxTrackSizingFunction::fit_content_percent(number(share)? / 100.0),
            None => MaxTrackSizingFunction::fit_content_px(number(limit)?),
        });
    }
    if let Some(share) = word.strip_suffix(t::FR) {
        return Ok(MaxTrackSizingFunction::fr(number(share)?));
    }
    Ok(match word {
        AUTO => MaxTrackSizingFunction::auto(),
        t::MIN_CONTENT => MaxTrackSizingFunction::min_content(),
        t::MAX_CONTENT => MaxTrackSizingFunction::max_content(),
        _ => match word.strip_suffix(t::PERCENT) {
            Some(share) => MaxTrackSizingFunction::percent(number(share)? / 100.0),
            None => MaxTrackSizingFunction::length(number(word)?),
        },
    })
}

/// One track: a size, a share, a content size, `fit_content` or `minmax`.
fn track(word: &str) -> Result<TrackSizingFunction, String> {
    if let Some(args) = call(word, t::MINMAX) {
        let [min, max] = args.as_slice() else {
            return Err(format!("`{word}` takes a minimum and a maximum"));
        };
        return Ok(TrackSizingFunction {
            min: low(min)?,
            max: high(max)?,
        });
    }
    let max = high(word)?;
    // A share alone is CSS's `minmax(auto, 1fr)`; anything else is both ends.
    let min = if max.is_fr() || max.is_fit_content() {
        MinTrackSizingFunction::auto()
    } else {
        low(word)?
    };
    Ok(TrackSizingFunction { min, max })
}

/// A `grid_columns` or `grid_rows` list; empty is none.
///
/// # Errors
/// The first word that does not read.
pub(crate) fn template(text: &str) -> Result<Vec<GridTemplateComponent<String>>, String> {
    words(text)
        .into_iter()
        .map(|word| {
            let Some(args) = call(word, t::REPEAT) else {
                return track(word).map(GridTemplateComponent::Single);
            };
            let Some((count, tracks)) = args.split_first() else {
                return Err(format!("`{word}` takes a count and tracks"));
            };
            let count = match *count {
                t::AUTO_FILL => RepetitionCount::AutoFill,
                t::AUTO_FIT => RepetitionCount::AutoFit,
                n => RepetitionCount::Count(
                    n.parse::<u16>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or_else(|| format!("`{n}` is not a count"))?,
                ),
            };
            let tracks = tracks
                .iter()
                .flat_map(|part| words(part))
                .map(track)
                .collect::<Result<Vec<_>, _>>()?;
            if tracks.is_empty() {
                return Err(format!("`{word}` repeats no track"));
            }
            Ok(GridTemplateComponent::Repeat(
                taffy::style::GridTemplateRepetition {
                    count,
                    tracks,
                    line_names: Vec::new(),
                },
            ))
        })
        .collect()
}

/// An `auto_columns` or `auto_rows` list, which takes no `repeat`.
///
/// # Errors
/// The first word that does not read.
pub(crate) fn auto_tracks(text: &str) -> Result<Vec<TrackSizingFunction>, String> {
    words(text).into_iter().map(track).collect()
}

/// How many columns a track list names, its counted `repeat`s spelled out
/// and an `auto_fill` or `auto_fit` as one pass of its tracks.
pub(crate) fn track_count(text: &str) -> usize {
    template(text).map_or(0, |tracks| {
        tracks
            .iter()
            .map(|track| match track {
                GridTemplateComponent::Single(_) => 1,
                GridTemplateComponent::Repeat(repeat) => match repeat.count {
                    RepetitionCount::Count(n) => usize::from(n) * repeat.tracks.len(),
                    RepetitionCount::AutoFill | RepetitionCount::AutoFit => repeat.tracks.len(),
                },
            })
            .sum()
    })
}

/// The named areas, one string a row; a blank row names nothing, which is
/// what a row the inspector has just added holds, and none is no areas.
///
/// # Errors
/// Rows of different lengths, or a name whose cells are not one rectangle.
pub(crate) fn areas(
    rows: &[smol_str::SmolStr],
) -> Result<Option<GridTemplateAreas<String>>, String> {
    let cells: Vec<Vec<&str>> = rows
        .iter()
        .map(|row| words(row))
        .filter(|row| !row.is_empty())
        .collect();
    if cells.is_empty() {
        return Ok(None);
    }
    let width = cells[0].len();
    if cells.iter().any(|row| row.len() != width) || width == 0 {
        return Err("every row names the same number of cells".into());
    }
    let mut found: Vec<GridTemplateArea<String>> = Vec::new();
    for (r, row) in cells.iter().enumerate() {
        for (c, name) in row.iter().enumerate() {
            if *name == t::EMPTY {
                continue;
            }
            let (r, c) = (r as u16, c as u16);
            match found.iter_mut().find(|area| area.name == *name) {
                Some(area) => {
                    area.row_start = area.row_start.min(r + 1);
                    area.row_end = area.row_end.max(r + 2);
                    area.column_start = area.column_start.min(c + 1);
                    area.column_end = area.column_end.max(c + 2);
                }
                None => found.push(GridTemplateArea {
                    name: (*name).to_string(),
                    row_start: r + 1,
                    row_end: r + 2,
                    column_start: c + 1,
                    column_end: c + 2,
                }),
            }
        }
    }
    for area in &found {
        for r in area.row_start - 1..area.row_end - 1 {
            for c in area.column_start - 1..area.column_end - 1 {
                if cells[usize::from(r)][usize::from(c)] != area.name {
                    return Err(format!("`{}` does not cover one rectangle", area.name));
                }
            }
        }
    }
    Ok(Some(GridTemplateAreas {
        areas: found,
        row_count: cells.len() as u16,
        column_count: width as u16,
    }))
}

/// One end of a placement: a line, a span, or a name.
fn end(word: &str) -> Result<GridPlacement<String>, String> {
    let word = word.trim();
    if word.is_empty() || word == AUTO {
        return Ok(GridPlacement::Auto);
    }
    if let Some(count) = word.strip_prefix(t::SPAN) {
        let count = count.trim();
        return count
            .parse::<u16>()
            .ok()
            .filter(|n| *n > 0)
            .map(GridPlacement::Span)
            .or_else(|| {
                (!count.is_empty() && !count.starts_with(|c: char| c.is_ascii_digit()))
                    .then(|| GridPlacement::NamedSpan(count.to_string(), 1))
            })
            .ok_or_else(|| format!("`{word}` spans no tracks"));
    }
    if let Ok(line) = word.parse::<i16>() {
        if line == 0 {
            return Err("lines count from 1, or from -1 at the end".into());
        }
        return Ok(GridPlacement::Line(line.into()));
    }
    if word.contains(char::is_whitespace) {
        return Err(format!("`{word}` is not a line, a span or a name"));
    }
    Ok(GridPlacement::NamedLine(word.to_string(), 1))
}

/// A child's `row` or `column`: one end, or two either side of a `/`. A name
/// alone covers its area, start line to end line.
///
/// # Errors
/// An end that does not read.
pub(crate) fn placement(text: &str) -> Result<Line<GridPlacement<String>>, String> {
    let (first, last) = if let Some((first, last)) = text.split_once('/') {
        (end(first)?, end(last)?)
    } else {
        let first = end(text)?;
        let last = match &first {
            GridPlacement::NamedLine(name, _) => GridPlacement::NamedLine(name.clone(), 1),
            _ => GridPlacement::Auto,
        };
        (first, last)
    };
    Ok(Line {
        start: first,
        end: last,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_list_reads_every_kind_of_track() {
        let tracks = template("120 1fr 25% auto min_content max_content fit_content(80) minmax(40, 2fr) repeat(3, 1fr 20)")
            .expect("every word reads");
        assert_eq!(tracks.len(), 9);
        assert_eq!(track_count("120 repeat(3, 1fr 20)"), 7);
        let GridTemplateComponent::Single(fixed) = &tracks[0] else {
            panic!("a size is one track");
        };
        assert_eq!(fixed.max, MaxTrackSizingFunction::length(120.0));
        let GridTemplateComponent::Single(share) = &tracks[1] else {
            panic!("a share is one track");
        };
        assert!(
            share.max.is_fr() && share.min.is_auto(),
            "1fr is minmax(auto, 1fr)"
        );
    }

    #[test]
    fn a_track_word_that_does_not_read_is_named() {
        assert!(template("1fr wide").unwrap_err().contains("wide"));
        assert!(template("minmax(1fr, 20)").unwrap_err().contains("minimum"));
        assert!(template("repeat(0, 1fr)").is_err());
        assert_eq!(template("").map(|t| t.len()), Ok(0));
    }

    #[test]
    fn areas_name_rectangles_and_refuse_anything_else() {
        let rows = ["head head".into(), "side main".into()];
        let grid = areas(&rows).expect("two rows of two").expect("named");
        let head = grid.areas.iter().find(|a| a.name == "head").unwrap();
        assert_eq!(
            (
                head.row_start,
                head.row_end,
                head.column_start,
                head.column_end
            ),
            (1, 2, 1, 3)
        );
        let bent = ["a a".into(), "a b".into()];
        assert!(areas(&bent).unwrap_err().contains("rectangle"));
        let ragged = ["a a".into(), "b".into()];
        assert!(areas(&ragged).is_err());
        let added = ["a a".into(), "".into()];
        assert!(areas(&added).is_ok_and(|grid| grid.is_some_and(|g| g.row_count == 1)));
    }

    #[test]
    fn a_placement_reads_lines_spans_and_names() {
        let both = placement("1 / 3").unwrap();
        assert_eq!(both.start, GridPlacement::Line(1.into()));
        assert_eq!(both.end, GridPlacement::Line(3.into()));
        assert_eq!(placement("span 2").unwrap().start, GridPlacement::Span(2));
        let named = placement("main").unwrap();
        assert_eq!(named.start, GridPlacement::NamedLine("main".into(), 1));
        assert_eq!(named.end, GridPlacement::NamedLine("main".into(), 1));
        assert_eq!(placement("").unwrap().start, GridPlacement::Auto);
        assert!(placement("0").is_err());
    }
}
