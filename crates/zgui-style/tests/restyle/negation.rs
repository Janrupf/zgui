//! A complex selector inside `:not()`, restyled as the ancestor and the attribute it reads move.

use crate::support::{Harness, color};

/// Sets attribute `name` of `node` to `value`.
fn set(harness: &mut Harness, node: zgui_dom::NodeIndex, name: &str, value: &str) {
    harness.edit(|edit| {
        edit.set_attribute(
            node,
            zgui_interned::AttrName::new(name),
            Some(zgui_vocab::SharedString::from(value)),
        );
    });
}

#[test]
fn a_descendant_combinator_inside_not_follows_the_ancestor_and_the_attribute() {
    let mut harness = Harness::new();
    let outer = harness.append(harness.root, "column");
    let button = harness.append(outer, "control");
    let late = harness.append(outer, "control");
    harness.set_classes(button, &["a"]);
    harness.set_classes(late, &["a"]);
    set(&mut harness, button, "data-v", "x");
    harness.add_author(
        "control { color: rgb(1, 1, 1) }
         .a.a[data-v=x]:not(.m *) { color: rgb(9, 0, 0) }",
    );
    harness.frame();
    assert_eq!(color(&harness, button), (9, 0, 0), "matches at first");
    assert_eq!(color(&harness, late), (1, 1, 1), "no attribute yet");
    harness.retire_all();

    harness.set_classes(outer, &["m"]);
    harness.frame();
    assert_eq!(
        color(&harness, button),
        (1, 1, 1),
        "an ancestor with `m` un-matches it"
    );
    harness.retire_all();

    harness.set_classes(outer, &[]);
    set(&mut harness, late, "data-v", "x");
    harness.frame();
    assert_eq!(color(&harness, button), (9, 0, 0), "matches again");
    assert_eq!(
        color(&harness, late),
        (9, 0, 0),
        "the attribute set later matches"
    );
}
