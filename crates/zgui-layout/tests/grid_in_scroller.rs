//! A grid that is a flex item of a scrolling column keeps its content height.
//!
//! The item's automatic minimum height is its min-content height, which for a grid is the height
//! of its rows sized to their content. The column then overflows and scrolls.

mod support;

use support::find::rect;
use support::{Element, Fixture, lay_out, measurer};

/// The fixture: a heading and a grid of five cells in a 260px scrolling column.
fn fixture(cell_css: &str) -> Fixture {
    let cell = |class: &'static [&'static str]| {
        Element::new("column").classes(class).children(vec![
            Element::new("box").classes(&["media"]),
            Element::new("text").text("Name"),
            Element::new("text").text("Footer"),
        ])
    };
    Fixture::new(
        Element::new("root").children(vec![
            Element::new("column").classes(&["scroller"]).children(vec![
                Element::new("text").text("Heading"),
                Element::new("box").classes(&["grid"]).children(vec![
                    cell(&["cell", "first"]),
                    cell(&["cell"]),
                    cell(&["cell"]),
                    cell(&["cell"]),
                    cell(&["cell"]),
                ]),
            ]),
        ]),
        &format!(
            "root {{ display: block; width: 600px }}
             .scroller {{ height: 260px; overflow-y: auto; gap: 8px }}
             .grid {{ display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 16px }}
             .cell {{ border: 1px solid red; {cell_css} }}
             .media {{ height: 100px; flex: none }}"
        ),
    )
}

/// Lays `fixture` out and checks that the cells and the grid keep their content height.
fn check(fixture: &Fixture) {
    let mut store = fixture.box_tree();
    let mut content = measurer();
    lay_out(&mut store, &mut content, 800.0, 600.0);

    // A border, the media box and two 24px lines.
    let (_, _, _, cell) = rect(&store, fixture, "first");
    assert_eq!(
        cell,
        1.0 + 100.0 + 24.0 + 24.0 + 1.0,
        "the cell keeps its text"
    );
    let (_, _, _, grid) = rect(&store, fixture, "grid");
    assert_eq!(
        grid,
        2.0 * cell + 16.0,
        "the grid is two rows of cells tall"
    );
}

#[test]
fn a_grid_of_visible_cells_keeps_its_height() {
    check(&fixture("overflow: visible"));
}

#[test]
fn a_grid_of_clipping_cells_keeps_its_height() {
    check(&fixture("overflow: hidden"));
}
