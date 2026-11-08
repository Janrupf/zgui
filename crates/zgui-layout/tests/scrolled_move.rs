//! A subtree that layout moved in the same frame its scrollport scrolled.
//!
//! The pass carries such a subtree by one offset: where layout put it plus how far the port
//! scrolled. Each box's composed shift records the scroll its contents were composed against, so
//! the carry has to record the scroll part as well. A box left with the old shift is offset by the
//! same scroll again the next time it is composed, and its clean contents are drawn that far from
//! their box.

mod support;

use support::{Element, Fixture, fragments, lay_out, lay_out_only, measurer};
use zgui_bits::Dirty;
use zgui_geom::{Device, DevicePx, Point};
use zgui_interned::ClassName;
use zgui_layout::fragment::diff::{Everything, FrameDirty};
use zgui_layout::{BoxKey, LayoutStore};

/// A scrollport holding a track moved by `top`, and a card of two lines on the track.
fn page(down: bool) -> Fixture {
    let track = Element::new("track").children(vec![Element::new("card").children(vec![
        Element::new("title").text("alpha bravo"),
        Element::new("line").text("delta gamma"),
    ])]);
    let track = if down {
        track.classes(&["down"])
    } else {
        track
    };
    Fixture::new(
        Element::new("root").children(vec![Element::new("port").children(vec![
            Element::new("above"),
            track,
            Element::new("below"),
        ])]),
        "root { display: block; width: 300px; height: 200px }
         port { display: block; height: 200px; overflow: scroll }
         above { display: block; height: 120px }
         below { display: block; height: 600px }
         track { display: block; position: relative; top: 0px }
         track.down { top: -24px }
         card { display: block; padding: 4px }
         title { display: block }
         line { display: block }",
    )
}

/// The box `path` names, as child indices from the root box.
fn box_at(store: &LayoutStore, path: &[usize]) -> BoxKey {
    let mut at = store.root().expect("a root box");
    for &index in path {
        at = store.node(at).children[index];
    }
    at
}

/// The element a box came from.
fn element(store: &LayoutStore, key: BoxKey) -> zgui_dom::NodeKey {
    store
        .get(key)
        .and_then(|node| node.source)
        .expect("the box came from an element")
}

/// Every text line's border box, in tree order.
fn lines(store: &LayoutStore) -> Vec<[f32; 4]> {
    let mut out = Vec::new();
    let mut stack = vec![store.root().expect("a root box")];
    while let Some(key) = stack.pop() {
        for frag in store.fragments_of_box(key) {
            let fragment = store.fragment(*frag).expect("a live fragment");
            if format!("{:?}", fragment.kind).starts_with("Line") {
                let rect = fragment.border_box;
                out.push([
                    rect.origin.x.0,
                    rect.origin.y.0,
                    rect.size.width.0,
                    rect.size.height.0,
                ]);
            }
        }
        stack.extend(store.node(key).children.iter().rev().copied());
    }
    out
}

/// What one frame owes: one element owes its own bits, and the elements above it owe them in
/// their subtrees.
struct Owes {
    /// The element that owes something.
    owner: zgui_dom::NodeKey,
    /// What it owes.
    bits: Dirty,
    /// It and every element above it.
    path: Vec<zgui_dom::NodeKey>,
}

impl Owes {
    /// `bits` owed by the element of `key`, with the chain above it read from `store`.
    fn at(store: &LayoutStore, key: BoxKey, bits: Dirty) -> Self {
        let mut path = Vec::new();
        let mut at = Some(key);
        while let Some(current) = at {
            if let Some(source) = store.get(current).and_then(|node| node.source) {
                path.push(source);
            }
            at = store.node(current).parent;
        }
        Self {
            owner: element(store, key),
            bits,
            path,
        }
    }
}

impl FrameDirty for Owes {
    fn own(&self, node: Option<zgui_dom::NodeKey>) -> Dirty {
        if node == Some(self.owner) {
            self.bits
        } else {
            Dirty::empty()
        }
    }

    fn subtree(&self, node: Option<zgui_dom::NodeKey>) -> Dirty {
        if node.is_some_and(|node| self.path.contains(&node)) {
            self.bits
        } else {
            Dirty::empty()
        }
    }

    fn mark(&mut self, _node: Option<zgui_dom::NodeKey>, _bits: Dirty) {}

    fn retire(&mut self, _node: Option<zgui_dom::NodeKey>, _phase: Dirty) {}
}

#[test]
fn a_subtree_moved_while_its_port_scrolled_keeps_its_lines_on_its_boxes() {
    // The document that took the long way: laid out, then moved and scrolled in one frame, then
    // one card repainted in the next.
    let mut fixture = page(false);
    let mut store = fixture.box_tree();
    let mut content = measurer();
    let mut frame = lay_out(&mut store, &mut content, 300.0, 200.0);
    let port_box = box_at(&store, &[0]);
    let port = element(&store, port_box);
    let track = {
        let document = fixture.document.store();
        let port = document.core(fixture.root).first_child().expect("the port");
        let above = document.core(port).first_child().expect("the spacer");
        document.core(above).next_sibling().expect("the track")
    };
    fixture.edit_restyle_and_patch(&mut store, |edit| {
        edit.set_classes(track, &[ClassName::new("down")]);
    });
    lay_out_only(&mut store, &mut content, 300.0, 200.0);
    frame.scroll.place(
        port,
        Point::<DevicePx, Device>::new(DevicePx(0.0), DevicePx(37.0)),
    );
    let root = store.root().expect("a root box");
    let mut scrolled = Owes::at(&store, port_box, Dirty::SCROLL);
    fragments(&mut frame, &mut store, root, &mut scrolled);
    let mut repainted = Owes::at(&store, box_at(&store, &[0, 1, 0]), Dirty::REPAINT);
    fragments(&mut frame, &mut store, root, &mut repainted);

    // The document that went there directly.
    let fresh_fixture = page(true);
    let mut fresh = fresh_fixture.box_tree();
    let mut fresh_content = measurer();
    let mut fresh_frame = lay_out(&mut fresh, &mut fresh_content, 300.0, 200.0);
    let fresh_port = element(&fresh, box_at(&fresh, &[0]));
    fresh_frame.scroll.place(
        fresh_port,
        Point::<DevicePx, Device>::new(DevicePx(0.0), DevicePx(37.0)),
    );
    let fresh_root = fresh.root().expect("a root box");
    fragments(&mut fresh_frame, &mut fresh, fresh_root, &mut Everything);

    let expected = lines(&fresh);
    assert_eq!(expected.len(), 2);
    assert_eq!(
        lines(&store),
        expected,
        "the card's lines are drawn away from the card they belong to"
    );
}
