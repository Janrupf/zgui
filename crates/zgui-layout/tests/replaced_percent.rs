//! A replaced box in a line resolves a percentage height against its containing block.
//!
//! An `image` is `inline-block`, so in a block container it sits in an anonymous inline root.
//! `height: 100%` resolves against the block container's height when that height is definite,
//! and the natural ratio applies only when the height stays `auto`.

mod support;

use support::find::{rect, with_class};
use support::{Element, Fixture, lay_out, lay_out_only, measurer_with_images};

/// A 160×90 picture in a frame.
fn fixture(css: &str) -> Fixture {
    Fixture::with_natural_size(
        Element::new("root").children(vec![Element::new("box").classes(&["frame"]).children(
            vec![Element::new("image").classes(&["pic"]).image(160.0, 90.0)],
        )]),
        css,
        (160.0, 90.0),
    )
}

/// The picture's border-box size after a layout of `fixture`.
fn picture(fixture: &Fixture) -> (f32, f32) {
    let mut store = fixture.box_tree();
    let mut content = measurer_with_images(160.0, 90.0);
    lay_out(&mut store, &mut content, 800.0, 600.0);
    let (_, _, width, height) = rect(&store, fixture, "pic");
    (width, height)
}

#[test]
fn a_percentage_height_fills_a_definite_frame() {
    let fixture = fixture(
        "root { display: block }
         .frame { width: 200px; height: 120px }
         .pic { width: 100%; height: 100% }",
    );
    assert_eq!(picture(&fixture), (200.0, 120.0));
}

#[test]
fn a_percentage_height_fills_a_frame_sized_by_its_ratio() {
    let fixture = fixture(
        "root { display: block }
         .frame { width: 200px; aspect-ratio: 1 }
         .pic { width: 100%; height: 100% }",
    );
    assert_eq!(picture(&fixture), (200.0, 200.0));
}

#[test]
fn a_percentage_height_in_an_auto_height_frame_keeps_the_natural_ratio() {
    let fixture = fixture(
        "root { display: block }
         .frame { width: 200px }
         .pic { width: 100%; height: 100% }",
    );
    let (width, height) = picture(&fixture);
    assert_eq!(width, 200.0);
    assert!(
        (height - 112.5).abs() <= 0.5,
        "an auto-height frame gives no basis, so the ratio decides: {height}"
    );
}

#[test]
fn the_picture_follows_a_change_of_the_frame_height() {
    let mut fixture = fixture(
        "root { display: block }
         .frame { width: 200px; height: 120px }
         .pic { width: 100%; height: 100% }",
    );
    let mut store = fixture.box_tree();
    let mut content = measurer_with_images(160.0, 90.0);
    lay_out(&mut store, &mut content, 800.0, 600.0);
    assert_eq!(rect(&store, &fixture, "pic").3, 120.0);

    let frame = with_class(&fixture, "frame");
    fixture.edit_restyle_and_patch(&mut store, |edit| {
        edit.set_style_property(frame, "height", Some("150px"));
    });
    lay_out_only(&mut store, &mut content, 800.0, 600.0);
    assert_eq!(rect(&store, &fixture, "pic").3, 150.0);
}

#[test]
fn a_restyle_to_a_percentage_height_takes_effect() {
    let mut fixture = fixture(
        "root { display: block }
         .frame { width: 200px; height: 120px }
         .pic { width: 100% }",
    );
    let mut store = fixture.box_tree();
    let mut content = measurer_with_images(160.0, 90.0);
    lay_out(&mut store, &mut content, 800.0, 600.0);
    let (_, _, _, before) = rect(&store, &fixture, "pic");
    assert!(
        (before - 112.5).abs() <= 0.5,
        "auto height keeps the ratio: {before}"
    );

    let pic = with_class(&fixture, "pic");
    fixture.edit_restyle_and_patch(&mut store, |edit| {
        edit.set_style_property(pic, "height", Some("100%"));
    });
    lay_out_only(&mut store, &mut content, 800.0, 600.0);
    assert_eq!(rect(&store, &fixture, "pic").3, 120.0);

    fixture.edit_restyle_and_patch(&mut store, |edit| {
        edit.set_style_property(pic, "height", None);
    });
    lay_out_only(&mut store, &mut content, 800.0, 600.0);
    let (_, _, _, after) = rect(&store, &fixture, "pic");
    assert!((after - 112.5).abs() <= 0.5, "back to auto: {after}");
}
