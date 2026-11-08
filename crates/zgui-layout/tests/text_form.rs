//! The shaper's laid-out form, which glyphs are painted from, agrees with the kept lines.
//!
//! A shaped paragraph holds one break at a time. The kept pass writes the lines a box paints, and
//! the paint reads its glyphs from that one break. A size probe that runs after the kept pass moves
//! the break, and the kept layout can come back from the engine's cache with no pass to move it
//! back. These tests pin that the layout pass ends with each form on its kept break.

mod support;

use std::sync::{Mutex, MutexGuard, PoisonError};

use cephal::tree::{LayoutInput, RequestedAxis, RunMode, SizingMode};
use cephal::{AvailableSpace, Line, Size};
use support::text::{first_inline_root, paragraph};
use support::{Content, Element, Fixture, lay_out_only, measurer};
use zgui_layout::{BoxKey, DeviceStyle, LayoutStore, LayoutTree, to_node_id};
use zgui_profile::{Counter, counter};

/// The counter block is process-wide, so cases that read it take turns.
fn exclusive() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How many lines the shaper's form holds for the paragraph `key` resolved to.
fn form_lines(store: &LayoutStore, content: &Content, key: BoxKey) -> usize {
    let resolution = store.inline_resolution(key).expect("laid out");
    content
        .cache()
        .get(resolution.key)
        .expect("the paragraph is shaped")
        .engine
        .lines
        .len()
}

/// How many lines `key` resolved to.
fn kept_lines(store: &LayoutStore, key: BoxKey) -> usize {
    store.inline_resolution(key).expect("laid out").lines.len()
}

/// Asks the engine for the size of `key` at `width`, as a parent's probe does.
fn probe(store: &mut LayoutStore, content: &mut Content, key: BoxKey, width: f32) {
    let mut tree = LayoutTree::new(store, content, DeviceStyle::default());
    let input = LayoutInput {
        run_mode: RunMode::ComputeSize,
        sizing_mode: SizingMode::InherentSize,
        axis: RequestedAxis::Both,
        known_dimensions: Size::NONE,
        known_dimensions_are_definite: Size::TRUE,
        parent_size: Size::NONE,
        available_space: Size {
            width: AvailableSpace::Definite(width),
            height: AvailableSpace::MaxContent,
        },
        vertical_margins_are_collapsible: Line::FALSE,
        context_key: 0,
    };
    cephal::compute::compute_child_layout(&mut tree, to_node_id(key), input, None);
}

/// One paragraph in a 400px block: two lines at 400px, three at 250px.
fn one_paragraph() -> Fixture {
    let text: &'static str = Box::leak(paragraph(12).into_boxed_str());
    Fixture::new(
        Element::new("root").children(vec![Element::new("para").text(text)]),
        "root { display: block }
         para { display: block }",
    )
}

#[test]
fn a_probe_after_the_kept_pass_leaves_the_glyphs_on_the_kept_lines() {
    let _guard = exclusive();
    let fixture = one_paragraph();
    let mut store = fixture.box_tree();
    let mut content = measurer();
    lay_out_only(&mut store, &mut content, 400.0, 4000.0);
    let key = first_inline_root(&store);
    assert_eq!(kept_lines(&store, key), 2, "the paragraph fills two lines");
    assert_eq!(form_lines(&store, &content, key), 2);

    // A probe at a narrower width moves the form. The next pass finds the kept layout in the
    // engine's cache and runs no kept pass over the paragraph.
    probe(&mut store, &mut content, key, 250.0);
    assert_eq!(
        form_lines(&store, &content, key),
        3,
        "the probe moved the form"
    );
    counter::reset();
    lay_out_only(&mut store, &mut content, 400.0, 4000.0);

    assert_eq!(kept_lines(&store, key), 2, "the kept lines stand");
    assert_eq!(
        form_lines(&store, &content, key),
        kept_lines(&store, key),
        "the glyphs are painted from a break the box was never laid out at"
    );
    assert_eq!(counter::get(Counter::TextFormsRestored), 1);
}

#[test]
fn a_steady_relayout_restores_nothing() {
    let _guard = exclusive();
    let fixture = one_paragraph();
    let mut store = fixture.box_tree();
    let mut content = measurer();
    lay_out_only(&mut store, &mut content, 400.0, 4000.0);
    let key = first_inline_root(&store);
    counter::reset();
    for _ in 0..3 {
        zgui_layout::tree::dirty::mark_dirty(&mut store, key);
        lay_out_only(&mut store, &mut content, 400.0, 4000.0);
    }
    assert_eq!(counter::get(Counter::TextFormsRestored), 0);
    assert_eq!(form_lines(&store, &content, key), 2);
}

#[test]
fn a_speculative_layout_writes_no_lines() {
    let _guard = exclusive();
    // A flex row aligned on baselines lays its items out in full to read their baselines, and a
    // size probe of the row does that under speculation: the engine keeps none of it.
    let text: &'static str = Box::leak(paragraph(12).into_boxed_str());
    let fixture = Fixture::new(
        Element::new("root").children(vec![Element::new("row").children(vec![
            Element::new("a").text(text),
            Element::new("b").text(text),
        ])]),
        "root { display: block }
         row { display: flex; align-items: baseline }
         a { display: block }
         b { display: block }",
    );
    let mut store = fixture.box_tree();
    let mut content = measurer();
    lay_out_only(&mut store, &mut content, 400.0, 4000.0);
    let items = support::text::inline_roots(&store);
    assert_eq!(items.len(), 2);
    let before: Vec<_> = items
        .iter()
        .map(|&key| {
            store
                .inline_resolution(key)
                .expect("laid out")
                .lines
                .clone()
        })
        .collect();
    let row = store.node(items[0]).parent.expect("a parent");
    let row = if store.inline_resolution(row).is_some() {
        row
    } else {
        store.node(row).parent.expect("the row")
    };

    probe(&mut store, &mut content, row, 200.0);
    let after: Vec<_> = items
        .iter()
        .map(|&key| {
            store
                .inline_resolution(key)
                .expect("laid out")
                .lines
                .clone()
        })
        .collect();
    assert_eq!(
        before, after,
        "a speculative layout replaced the kept lines"
    );

    lay_out_only(&mut store, &mut content, 400.0, 4000.0);
    for &key in &items {
        assert_eq!(form_lines(&store, &content, key), kept_lines(&store, key));
    }
}

#[test]
fn a_gutter_that_appears_leaves_every_title_on_its_kept_lines() {
    let _guard = exclusive();
    // A board of columns, each a scrolling list of cards. Cards inserted at the front make the
    // first list overflow and its gutter appear. The column is laid out again on its own, then the
    // row around the board measures the board at its max-content width, which probes every title
    // at a wider width. The board's kept layout gives the column the width it already has, so the
    // column is a cache hit and no kept pass moves the titles' forms back.
    let titles = [
        "Interview five customers about pricing",
        "Interview five customers about billing",
        "Interview five customers about routing",
        "Interview five customers about hosting",
        "Interview five customers about testing",
        "Interview five customers about staging",
    ];
    let card = |title: &'static str| {
        Element::new("card").children(vec![Element::new("title").text(title)])
    };
    let column = |cards: Vec<Element>| {
        Element::new("column").children(vec![Element::new("area").children(vec![
            Element::new("viewport").children(vec![Element::new("list").children(cards)]),
        ])])
    };
    let mut fixture = Fixture::new(
        Element::new("root").children(vec![Element::new("board").children(vec![
            column(vec![card(titles[0]), card(titles[1])]),
            column(vec![card(titles[0])]),
            column(vec![card(titles[1])]),
            column(vec![card(titles[2])]),
        ])]),
        "root { display: flex }
         board { display: flex; flex: 1 1 auto; min-width: 0; gap: 16px; height: 300px }
         column { display: flex; flex-direction: column; flex: 1 1 0; min-width: 0; min-height: 0 }
         area { display: flex; overflow: hidden; flex: 1 1 auto; min-height: 0 }
         viewport { display: block; flex: 1 1 auto; overflow: auto; width: 100%; height: 100% }
         list { display: flex; flex-direction: column }
         card { display: flex; flex-direction: column; padding: 6px; border: 1px solid }
         title { display: block }",
    );
    let mut store = fixture.box_tree();
    let mut content = measurer();
    lay_out_only(&mut store, &mut content, 800.0, 600.0);

    let list = {
        let document = fixture.document.store();
        let mut at = fixture.root;
        for _ in 0..5 {
            at = document.core(at).first_child().expect("one level down");
        }
        at
    };
    for title in &titles[2..] {
        let first = fixture
            .document
            .store()
            .core(list)
            .first_child()
            .expect("a card");
        fixture.edit_and_sync_boxes(&mut store, |edit| {
            let card = edit.create_element(zgui_interned::ElementName::new("card"));
            let heading = edit.create_element(zgui_interned::ElementName::new("title"));
            let text = edit.create_text(title);
            edit.insert_before(heading, text, None);
            edit.insert_before(card, heading, None);
            edit.insert_before(list, card, Some(first));
        });
        lay_out_only(&mut store, &mut content, 800.0, 600.0);
    }

    let titles = support::text::inline_roots(&store);
    assert_eq!(titles.len(), 9);
    let lines: Vec<usize> = titles.iter().map(|&key| kept_lines(&store, key)).collect();
    assert!(
        lines.contains(&3),
        "the gutter rewrapped no title: {lines:?}"
    );
    for &key in &titles {
        assert_eq!(
            form_lines(&store, &content, key),
            kept_lines(&store, key),
            "a title paints the lines of a width it was not laid out at"
        );
    }
}
