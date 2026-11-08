//! The min-content width of a wrapping flex row is its widest item, so the row can shrink and
//! wrap (CSS Flexible Box Layout, section 9.9.1).

mod support;

use support::find::rect;
use support::{Element, Fixture, lay_out, measurer};

#[test]
fn a_wrapping_row_lets_its_flex_parent_shrink() {
    let tag = |class: &'static [&'static str], text| Element::new("text").classes(class).text(text);
    let fixture = Fixture::new(
        Element::new("root").children(vec![Element::new("row").classes(&["outer"]).children(
            vec![
                Element::new("column").classes(&["main"]).children(vec![
                    Element::new("row").classes(&["tags"]).children(vec![
                        tag(&["first"], "alpha one"),
                        tag(&[], "beta two"),
                        tag(&[], "gamma three"),
                        tag(&[], "delta four"),
                    ]),
                ]),
                Element::new("box").classes(&["side"]),
            ],
        )]),
        "root { display: block }
         .outer { width: 480px; flex-wrap: wrap; gap: 16px }
         .main { flex: 1 1 0 }
         .tags { flex-wrap: wrap; gap: 24px }
         .side { flex: none; width: 200px; height: 40px }",
    );
    let mut store = fixture.box_tree();
    let mut content = measurer();
    lay_out(&mut store, &mut content, 800.0, 600.0);

    let (main_x, main_y, main_width, _) = rect(&store, &fixture, "main");
    let (side_x, side_y, _, _) = rect(&store, &fixture, "side");
    let (_, _, _, tags_height) = rect(&store, &fixture, "tags");
    let (_, _, _, tag_height) = rect(&store, &fixture, "first");
    assert_eq!(
        main_y, side_y,
        "the side box stays on the line of the main column"
    );
    assert_eq!(
        main_width, 264.0,
        "the main column takes what the side box leaves"
    );
    assert_eq!(side_x, main_x + 264.0 + 16.0);
    assert!(
        tags_height > tag_height,
        "the tags wrap onto more than one line: {tags_height} tall, one tag {tag_height}"
    );
}
