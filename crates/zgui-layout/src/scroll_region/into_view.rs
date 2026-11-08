//! Where a box sits inside the scroll container that brings it into view, read from layout.
//!
//! Answered from the resolved layout alone, so it is available as soon as a layout pass has run
//! and before the fragment pass composes anything: a box created in this frame has a place here
//! already. The answer ignores sticky offsets and transforms, which the fragment pass applies.

use zgui_dom::side::BoxKey;
use zgui_geom::{Device, DevicePx, Point, Rect, Size};

use crate::tree::store::LayoutStore;

/// `target`'s border box in `container`'s unscrolled content space, and the size of the
/// container's scrollport.
///
/// The content space has its origin at the container's content-box corner with no scroll
/// offset applied, so the visible part of it is the scrollport moved by the offset.
///
/// Answers with nothing when `target` is not below `container` in the box tree, when a box on
/// the way has not been laid out, or when a box on the way is placed by line layout rather than
/// by an origin of its own — an inline box or a text run. A caller that has fragments for such a
/// box measures them instead.
pub fn target_in(
    store: &LayoutStore,
    container: BoxKey,
    target: BoxKey,
) -> Option<(Rect<DevicePx, Device>, Size<DevicePx, Device>)> {
    let size = store.layout_of(target)?.size;
    let (mut x, mut y) = (0.0, 0.0);
    let mut key = target;
    while key != container {
        let node = store.get(key)?;
        if node.is_nested_inline() {
            return None;
        }
        let layout = store.layout_of(key)?;
        x += layout.origin.x.0;
        y += layout.origin.y.0;
        key = node.parent?;
    }
    let port = store.layout_of(container)?;
    x -= port.border.left.0 + port.padding.left.0;
    y -= port.border.top.0 + port.padding.top.0;
    Some((
        Rect::new(Point::new(DevicePx(x), DevicePx(y)), size),
        port.content_box().size,
    ))
}
