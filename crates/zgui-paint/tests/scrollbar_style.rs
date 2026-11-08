//! What a scroll container's style says about its bars: the scheme and the colours.

mod support;

use support::{Element, Harness};
use zgui_paint::lower::scrollbar::ScrollbarScheme;

/// The lowered paint style of a named element.
fn style_of(harness: &Harness, name: &str) -> zgui_paint::lower::PaintStyle {
    let key = harness.box_of(name);
    zgui_paint::lower::lower(&harness.store().node(key).style, 1.0)
}

/// A root holding a scroller and a plain box.
fn tree() -> Element {
    Element::new("root").children(vec![Element::new("column"), Element::new("mark")])
}

/// The sheet every case shares, with `extra` after it.
fn sheet(extra: &str) -> String {
    format!(
        "root {{ display: block; width: 200px; height: 200px }}
         column {{ display: block; height: 50px; overflow-y: auto }}
         mark {{ display: block; height: 20px }}
         {extra}"
    )
}

#[test]
fn an_inherited_dark_scheme_reaches_a_scroller_and_no_other_box() {
    let harness = Harness::new(tree(), &sheet("root { color-scheme: dark }"));
    assert_eq!(
        style_of(&harness, "column").scrollbar.scheme,
        ScrollbarScheme::Dark
    );
    assert_eq!(
        style_of(&harness, "mark").scrollbar,
        zgui_paint::lower::ScrollbarStyle::default(),
        "a box that does not scroll reads nothing for its bars"
    );
}

#[test]
fn a_scheme_naming_both_or_none_follows_the_window() {
    let harness = Harness::new(tree(), &sheet("root { color-scheme: light dark }"));
    assert_eq!(
        style_of(&harness, "column").scrollbar.scheme,
        ScrollbarScheme::Window
    );
    let harness = Harness::new(tree(), &sheet(""));
    let style = style_of(&harness, "column").scrollbar;
    assert_eq!(style.scheme, ScrollbarScheme::Window);
    assert!(style.colors.is_none(), "nothing set, nothing allocated");
}

#[test]
fn the_custom_properties_set_the_thumb_and_the_track() {
    let harness = Harness::new(
        tree(),
        &sheet(
            ":root { --zgui-scrollbar-thumb: rgb(0, 255, 0) }
             column { --zgui-scrollbar-track: rgb(0, 0, 255) }",
        ),
    );
    let colors = style_of(&harness, "column")
        .scrollbar
        .colors
        .expect("the colours were read");
    assert_eq!(
        colors.thumb.map(|c| c.to_premultiplied_srgb()),
        Some([0.0, 1.0, 0.0, 1.0])
    );
    assert_eq!(
        colors.track.map(|c| c.to_premultiplied_srgb()),
        Some([0.0, 0.0, 1.0, 1.0])
    );
    assert!(style_of(&harness, "mark").scrollbar.colors.is_none());
}
