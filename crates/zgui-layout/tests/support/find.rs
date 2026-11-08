//! Finding an element of a fixture by class, and reading the box it generated.

use zgui_dom::{NodeIndex, NodeKind};
use zgui_layout::tree::store::LayoutStore;

use super::Fixture;

/// The first element, in tree order, that carries `class`.
///
/// # Panics
///
/// Panics when no element carries the class.
pub(crate) fn with_class(fixture: &Fixture, class: &str) -> NodeIndex {
    let store = fixture.document.store();
    let mut stack = vec![fixture.root];
    while let Some(index) = stack.pop() {
        if store.core(index).kind() == NodeKind::Element
            && store.classes_of(index).iter().any(|name| &**name == class)
        {
            return index;
        }
        let mut children = Vec::new();
        let mut next = store.core(index).first_child();
        while let Some(child) = next {
            children.push(child);
            next = store.core(child).next_sibling();
        }
        stack.extend(children.into_iter().rev());
    }
    panic!("the fixture has no element with class `{class}`");
}

/// The border box `(x, y, width, height)` of the first box that the element with `class`
/// generated, relative to its parent box, in device pixels.
pub(crate) fn rect(store: &LayoutStore, fixture: &Fixture, class: &str) -> (f32, f32, f32, f32) {
    let key = fixture.document.store().key_of(with_class(fixture, class));
    let box_ = *store
        .boxes_of(key)
        .first()
        .unwrap_or_else(|| panic!("`.{class}` generated no box"));
    let layout = store
        .layout_of(box_)
        .unwrap_or_else(|| panic!("`.{class}` has no layout"));
    let border = layout.border_box();
    (
        border.origin.x.0,
        border.origin.y.0,
        border.size.width.0,
        border.size.height.0,
    )
}
