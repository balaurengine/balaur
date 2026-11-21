//! What holds for every component beside `widget`: each row has its one
//! place, and no kind reads one row from two components.

use balaur_core::components::derived::Fields;

use crate::vocabulary::words as w;
use crate::widget::beside::{
    Beside, WidgetCode, WidgetColorPicker, WidgetDropdown, WidgetField, WidgetImage, WidgetList,
    WidgetMenu, WidgetRange, WidgetScroll, WidgetSeparator, WidgetTable, WidgetWindow, each_beside,
    owners_of,
};
use crate::widget::beside_box::{
    WidgetBox, WidgetChild, WidgetFold, WidgetGrid, WidgetLayout, WidgetTabs,
};
use crate::widget::beside_controls::{
    WidgetButton, WidgetCheck, WidgetRows, WidgetShortcut, WidgetText,
};
use crate::widget::properties::WidgetProperties;
use crate::widget::schema::{changed_value, widget_of};

/// Each row written alone leaves the widget as putting the component
/// whole does: a row the table forgot would be left at its field's zero.
fn each_row_fills_what_a_whole_put_fills<C: Beside>(kind: &str) {
    let asked = WidgetProperties {
        kind: crate::widget::properties::Kind::new(kind),
        ..WidgetProperties::default()
    };
    for (index, row) in C::NAMES.iter().enumerate() {
        let mut held = C::default();
        let mut alone = widget_of(&asked);
        C::default().put(&mut alone);
        let changed = changed_value::<C>(&held, index);
        held.write(index, (&changed).into())
            .expect("the value fits the row");
        assert!(
            held.put_row(&mut alone, row),
            "`{row}` of `{}` has no place",
            C::NAME
        );
        let mut whole = widget_of(&asked);
        held.put(&mut whole);
        assert_eq!(
            format!("{alone:?}"),
            format!("{whole:?}"),
            "`{row}` of `{}` written alone is not what a whole put leaves",
            C::NAME
        );
    }
}

#[test]
fn a_row_written_alone_fills_what_a_whole_put_fills() {
    macro_rules! check_each {
        (; $($beside:ty),*) => {
            $(each_row_fills_what_a_whole_put_fills::<$beside>(
                w::WIDGET_KINDS
                    .iter()
                    .find(|kind| <$beside>::reads(kind))
                    .expect("some kind reads every component"),
            );)*
        };
    }
    each_beside!(check_each!());
}

// A control's table says nothing of which component holds a row,
// so no kind may read two that hold the same one.
#[test]
fn no_kind_reads_two_components_holding_one_row() {
    macro_rules! rows_of_each {
        (; $($beside:ty),*) => {
            [$(<$beside as Fields>::NAMES),*]
        };
    }
    for rows in each_beside!(rows_of_each!()) {
        for row in rows {
            for kind in w::WIDGET_KINDS {
                let readers: Vec<&str> = owners_of(row)
                    .iter()
                    .filter(|owner| (owner.reads)(kind))
                    .map(|owner| owner.name)
                    .collect();
                assert!(
                    readers.len() <= 1,
                    "a `{kind}` reads `{row}` from each of {readers:?}"
                );
            }
        }
    }
}

// No component holds a row of `widget` itself, or a control's
// table could not say which it meant.
#[test]
fn no_row_beside_the_widget_is_a_row_of_the_widget() {
    for row in WidgetProperties::NAMES {
        assert!(
            owners_of(row).is_empty(),
            "`{row}` is on `widget` and on `{}`",
            owners_of(row)[0].name
        );
    }
}

#[test]
fn a_container_is_built_without_the_scroll_rows_it_seldom_reads() {
    for kind in [w::ROW, w::COLUMN, w::GRID, w::PANEL] {
        assert!(
            WidgetScroll::reads(kind) && !WidgetScroll::defaults(kind),
            "a `{kind}` reads `widget_scroll` and is not handed its defaults"
        );
    }
    assert!(WidgetScroll::defaults(w::SCROLL));
}

#[test]
fn a_box_that_scrolls_reads_the_rows_defaults_until_its_node_states_them() {
    use std::borrow::Cow;
    let asked = WidgetProperties {
        kind: crate::widget::properties::Kind::new(w::COLUMN),
        ..WidgetProperties::default()
    };
    let bare = widget_of(&asked);
    let read = crate::widget::scroll::rows_of(&bare);
    assert!(
        matches!(read, Cow::Owned(_)),
        "a bare column is read as it is"
    );
    assert!(
        read.axis == w::BOTH
            && read.egui.wheel_scroll
            && read.egui.scroll_offset.iter().all(|at| *at < 0.0)
    );
    let mut stated = widget_of(&asked);
    WidgetScroll {
        wheel_scroll: false,
        ..WidgetScroll::default()
    }
    .put(&mut stated);
    let read = crate::widget::scroll::rows_of(&stated);
    assert!(matches!(read, Cow::Borrowed(_)) && !read.egui.wheel_scroll);
}
