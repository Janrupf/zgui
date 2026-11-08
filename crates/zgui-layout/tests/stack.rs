//! `stack` puts its children over one another, as its documentation says.

mod support;

use support::find::rect;
use support::{Element, Fixture, lay_out, measurer};

#[test]
fn stack_children_share_one_origin() {
    let fixture = Fixture::new(
        Element::new("root").children(vec![Element::new("stack").classes(&["stack"]).children(
            vec![
                Element::new("box").classes(&["back"]),
                Element::new("box").classes(&["front"]),
            ],
        )]),
        "root { display: block; width: 400px }
         .back { height: 20px }
         .front { height: 40px }",
    );
    let mut store = fixture.box_tree();
    let mut content = measurer();
    lay_out(&mut store, &mut content, 400.0, 300.0);

    let (bx, by, _, bh) = rect(&store, &fixture, "back");
    let (fx, fy, _, fh) = rect(&store, &fixture, "front");
    let (_, _, _, stack_height) = rect(&store, &fixture, "stack");
    assert_eq!((bx, by), (fx, fy), "both children start at the same point");
    assert_eq!((bh, fh), (20.0, 40.0));
    assert_eq!(
        stack_height, 40.0,
        "the stack is as tall as its tallest child"
    );
}

#[test]
fn stack_is_the_containing_block_of_an_absolute_child() {
    let fixture = Fixture::new(
        Element::new("root").children(vec![
            Element::new("box").classes(&["spacer"]),
            Element::new("stack").classes(&["stack"]).children(vec![
                Element::new("box").classes(&["base"]),
                Element::new("box").classes(&["badge"]),
            ]),
        ]),
        "root { display: block; width: 400px }
         .spacer { height: 50px }
         .stack { width: 100px; height: 60px }
         .base { height: 60px }
         .badge { position: absolute; right: 0; bottom: 0; width: 10px; height: 10px }",
    );
    let mut store = fixture.box_tree();
    let mut content = measurer();
    lay_out(&mut store, &mut content, 400.0, 300.0);

    let (x, y, _, _) = rect(&store, &fixture, "badge");
    assert_eq!(
        (x, y),
        (90.0, 50.0),
        "the badge sits in the stack's bottom right corner"
    );
}
