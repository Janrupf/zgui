//! The colour a box's border is stroked with, one per side where the sides differ.

mod support;

use support::{Element, Harness};
use zgui_color::Color;
use zgui_scene::Paint;

/// The stroke paint of the one bordered quad `css` gives `mark`.
fn stroke(css: &str) -> Option<Paint> {
    let sheet = format!(
        "root {{ display: block; width: 200px; height: 100px }}
         mark {{ display: block; width: 100px; height: 40px; {css} }}"
    );
    let mut harness = Harness::new(
        Element::new("root").children(vec![Element::new("mark")]),
        Box::leak(sheet.into_boxed_str()),
    );
    harness.paint_everything();
    let scene = harness.scene();
    let quad = scene
        .primitives
        .quads
        .iter()
        .find(|quad| quad.border.iter().any(|width| *width > 0.0))
        .expect("a bordered quad");
    scene.paints.get(quad.stroke.id()?).cloned()
}

fn srgb(red: f32, green: f32, blue: f32, alpha: f32) -> Color {
    Color::srgb(red, green, blue, alpha)
}

#[test]
fn a_side_of_another_colour_strokes_each_side_in_its_own() {
    let blue = srgb(0.0, 0.0, 1.0, 1.0);
    let red = srgb(1.0, 0.0, 0.0, 1.0);
    assert_eq!(
        stroke("border: 1px solid blue; border-left: 4px solid red"),
        Some(Paint::Sides([blue, blue, blue, red]))
    );
}

#[test]
fn a_transparent_border_with_one_coloured_side_keeps_the_others_transparent() {
    let Some(Paint::Sides(colors)) =
        stroke("border: 1px solid transparent; border-left-color: red")
    else {
        panic!("one visible side among transparent ones is a sides stroke");
    };
    assert_eq!(colors[0].alpha(), 0.0);
    assert_eq!(colors[3], srgb(1.0, 0.0, 0.0, 1.0));
}

#[test]
fn one_colour_on_every_side_with_a_width_is_a_solid_stroke() {
    // Only the left side has a width, so the others' colours do not count.
    assert_eq!(
        stroke("color: blue; border-left: 4px solid red"),
        Some(Paint::Solid(srgb(1.0, 0.0, 0.0, 1.0)))
    );
    assert_eq!(
        stroke("border: 2px solid red"),
        Some(Paint::Solid(srgb(1.0, 0.0, 0.0, 1.0)))
    );
}
