//! A scroll a view asks for in the flush that changes what it scrolls to.
//!
//! The request is carried out after the frame's layout pass, so it measures the layout that holds
//! what the same flush added: a row appended to a log, or the log itself on first mount.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use zgui_geom::{DevicePx, Point};
use zgui_reactive::prelude::{Get, GetUntracked, Set, Track, Update};
use zgui_reactive::{RenderEffect, RwSignal};
use zgui_view::{BuildCx, IntoView, NodeRef, ScrollBehavior, ScrollTarget, View, ViewHost};

/// A list of 20px rows in a 200px scrollport.
const CSS: &str = "
root { display: block; width: 400px; height: 300px }
.list { display: block; width: 400px; height: 200px; overflow-y: auto }
.row { display: block; height: 20px }
.end { display: block; height: 1px }
";

/// The application under test.
type Harness = zgui_platform_headless::Harness<zgui_runtime::Runtime>;

/// The rows signal the view made, handed out of the build closure.
type Rows = Rc<RefCell<Option<RwSignal<Vec<u32>, zgui_reactive::LocalStorage>>>>;

/// A list of rows. With `follow`, an effect scrolls the end marker into view whenever the rows
/// change, in the same flush that changes them.
fn listing(follow: bool) -> (Harness, Rows) {
    let slot: Rows = Rc::default();
    let handed = Rc::clone(&slot);
    let harness = support::app_with_text(CSS, move |cx: &mut BuildCx<'_>| {
        let rows = RwSignal::new_local((0..40).collect::<Vec<u32>>());
        *handed.borrow_mut() = Some(rows);
        let end = NodeRef::new();
        if follow {
            core::mem::forget(RenderEffect::new(move |_| {
                rows.track();
                if end.get().is_some() {
                    end.scroll_to(ScrollTarget::IntoViewEnd, ScrollBehavior::Instant);
                }
            }));
        }
        let list = zgui_elements::column()
            .class("list")
            .child(zgui_view::For::new(
                move || rows.get(),
                |n: &u32| *n,
                |_| zgui_view::AnyView::new(zgui_elements::column().class("row")),
            ))
            .child(zgui_elements::column().class("end").node_ref(end));
        Box::new(
            zgui_elements::column()
                .class("root")
                .child(list)
                .into_view()
                .build(cx),
        )
    });
    (harness, slot)
}

/// The scroll container, and how far it is scrolled down.
fn scrolled(harness: &Harness) -> f32 {
    let window = harness.app().windows().first().expect("a window");
    let layout = window.layout().borrow();
    let element = layout
        .keys()
        .into_iter()
        .find(|key| zgui_layout::scroll_region::region_of(&layout, *key).is_some())
        .and_then(|key| layout.node(key).source)
        .expect("the list scrolls");
    window.scroll().borrow().offset_of(element).y.0
}

/// How far the list can scroll: its rows and end marker less its 200px scrollport.
fn limit(rows: usize) -> f32 {
    rows as f32 * 20.0 + 1.0 - 200.0
}

#[test]
fn a_list_that_follows_its_end_opens_at_the_end_and_follows_each_new_row() {
    let (mut harness, rows) = listing(true);
    harness.settle(8);
    assert_eq!(
        scrolled(&harness),
        limit(40),
        "the first mount opens at the end"
    );

    let rows = rows.borrow().expect("the view was built");
    rows.update(|rows| rows.push(40));
    harness.settle(8);
    assert_eq!(
        scrolled(&harness),
        limit(41),
        "the scroll measured the layout holding the new row"
    );
}

#[test]
fn an_offset_past_the_old_extent_lands_at_the_new_limit() {
    let (mut harness, rows) = listing(false);
    harness.settle(8);
    assert_eq!(scrolled(&harness), 0.0);
    let rows = rows.borrow().expect("the view was built");
    let list = {
        let window = harness.app().windows().first().expect("a window");
        let layout = window.layout().borrow();
        layout
            .keys()
            .into_iter()
            .find(|key| zgui_layout::scroll_region::region_of(&layout, *key).is_some())
            .and_then(|key| layout.node(key).source)
            .expect("the list scrolls")
    };

    // Ten more rows, and an offset that only the grown list can reach, asked for in one flush.
    rows.update(|rows| rows.extend(40..50));
    harness.app().windows()[0].host().scroll_to(
        zgui_view_dom::id::to_view(list),
        ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(10_000.0))),
        ScrollBehavior::Instant,
    );
    harness.settle(8);
    assert_eq!(scrolled(&harness), limit(50));
}

/// A window whose own root scrolls.
const PAGE_CSS: &str = "
root { display: block; width: 400px; height: 300px; overflow-y: scroll }
.row { display: block; height: 20px }
";

#[test]
fn a_freeze_in_the_same_flush_as_a_scroll_applies_first() {
    let mut harness = support::app_with_text(PAGE_CSS, move |cx: &mut BuildCx<'_>| {
        let mut page = zgui_elements::column();
        for _ in 0..40 {
            page = page.child(zgui_elements::column().class("row"));
        }
        Box::new(page.into_view().build(cx))
    });
    harness.settle(8);
    let root = {
        let window = harness.app().windows().first().expect("a window");
        let layout = window.layout().borrow();
        let key = layout.root().expect("a root box");
        layout.node(key).source.expect("the root element")
    };
    let page_offset = |harness: &Harness| {
        let window = harness.app().windows().first().expect("a window");
        window.scroll().borrow().offset_of(root).y.0
    };
    let down = ScrollTarget::Offset(Point::new(DevicePx(0.0), DevicePx(100.0)));

    // Asked for before the freeze, in the same flush: the scroll waits for the layout pass, and
    // the freeze does not, so the window stays where it is.
    let host = harness.app().windows()[0].host().clone();
    host.scroll_to(
        zgui_view_dom::id::to_view(root),
        down,
        ScrollBehavior::Instant,
    );
    host.freeze_scrolling(true);
    harness.settle(8);
    assert_eq!(page_offset(&harness), 0.0, "the frozen window did not move");

    host.freeze_scrolling(false);
    host.scroll_to(
        zgui_view_dom::id::to_view(root),
        down,
        ScrollBehavior::Instant,
    );
    harness.settle(8);
    assert_eq!(page_offset(&harness), 100.0);
}

/// A list with no height of its own until a class caps it.
const CAPPED_CSS: &str = "
root { display: block; width: 400px; height: 300px }
.list { display: block; width: 400px; overflow-y: auto }
.capped { height: 200px }
.row { display: block; height: 20px }
";

#[test]
fn a_scroll_into_view_measures_the_relayout_an_observation_asks_for() {
    // As a popper does: the list opens uncapped, its measured size is delivered, and the reader
    // caps it in the same frame. The scroll was asked for in the flush that mounted the list.
    let mut harness = support::app_with_text(CAPPED_CSS, move |cx: &mut BuildCx<'_>| {
        let list = NodeRef::new();
        let target = NodeRef::new();
        let capped = RwSignal::new_local(false);
        let size = list.observe_border_size();
        core::mem::forget(RenderEffect::new(move |_| {
            if size.get().is_some_and(|size| size.height.0 > 200.0) && !capped.get_untracked() {
                capped.set(true);
            }
        }));
        core::mem::forget(RenderEffect::new(move |_| {
            if target.get().is_some() {
                target.scroll_to(ScrollTarget::IntoView, ScrollBehavior::Instant);
            }
        }));
        let mut column = zgui_elements::column()
            .class("list")
            .class_toggle(zgui_view::ClassName::new("capped"), move || capped.get())
            .node_ref(list);
        for index in 0..40 {
            let row = zgui_elements::column().class("row");
            column = column.child(if index == 30 {
                row.node_ref(target)
            } else {
                row
            });
        }
        Box::new(
            zgui_elements::column()
                .class("root")
                .child(column)
                .into_view()
                .build(cx),
        )
    });
    harness.settle(8);
    // Row 30 spans 600px to 620px, so the 200px scrollport ends at its bottom edge.
    assert_eq!(scrolled(&harness), 420.0);
}
