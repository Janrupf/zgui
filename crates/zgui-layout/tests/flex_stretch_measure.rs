//! A stretched item in a column whose width is indefinite is measured at the column's
//! fit-content width.
//!
//! A month grid of square days in a centred column is as tall as its rows at the width it gets,
//! and the column around it reserves no more.

mod support;

use support::find::rect;
use support::{Element, Fixture, lay_out, measurer};

#[test]
fn a_month_grid_in_a_centred_column_is_as_tall_as_its_rows() {
    let days = (0..42)
        .map(|_| Element::new("box").classes(&["day"]))
        .collect();
    let fixture = Fixture::new(
        Element::new("root").children(vec![Element::new("column").classes(&["page"]).children(
            vec![Element::new("column").classes(&["calendar"]).children(vec![
                Element::new("box").classes(&["grid"]).children(days),
            ])],
        )]),
        "root { display: block }
         .page { width: 800px; align-items: center }
         .grid { display: grid; grid-template-columns: repeat(7, minmax(32px, 1fr)) }
         .day { aspect-ratio: 1 / 1; width: 100% }",
    );
    let mut store = fixture.box_tree();
    let mut content = measurer();
    lay_out(&mut store, &mut content, 1000.0, 800.0);

    let (_, _, width, height) = rect(&store, &fixture, "calendar");
    assert_eq!((width, height), (7.0 * 32.0, 6.0 * 32.0));
}
