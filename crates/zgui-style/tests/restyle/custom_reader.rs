//! A custom element reads custom properties in its own `layout`, out of reach of its rule chains.
//! A change above it reaches it all the same.

use crate::support::Harness;

/// The computed text of `--c` on `node`.
fn custom_c(harness: &Harness, node: zgui_dom::NodeIndex) -> Option<String> {
    let style = harness
        .document
        .node(node)
        .primary_style()
        .expect("the element is styled");
    zgui_css::values::custom::text(&style, "c").map(str::to_owned)
}

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
fn an_attribute_on_the_parent_restyles_a_custom_child_that_names_no_var() {
    let mut harness = Harness::new();
    let parent = harness.append(harness.root, "column");
    let custom = harness.append(parent, zgui_vocab::prop::custom::TAG);
    let plain = harness.append(parent, "box");
    harness.set_classes(parent, &["r"]);
    set(&mut harness, parent, "data-x", "a");
    harness.add_author(
        ".r[data-x=a] { --c: red }
         .r[data-x=b] { --c: blue }",
    );
    harness.frame();
    assert_eq!(custom_c(&harness, custom).as_deref(), Some("red"));
    harness.retire_all();

    set(&mut harness, parent, "data-x", "b");
    let pass = harness.frame();
    assert_eq!(
        custom_c(&harness, custom).as_deref(),
        Some("blue"),
        "the custom element took the new value in the frame of the change"
    );
    let styled = pass.styled_nodes();
    assert!(
        styled.contains(&custom),
        "and it was restyled, so its box takes the new style: {styled:?}"
    );
    assert!(
        !styled.contains(&plain),
        "a sibling that reads nothing is still skipped"
    );
}
