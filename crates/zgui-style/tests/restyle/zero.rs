//! Keyframe animations with a duration of `0s`.
//!
//! An animation of no length has an active interval of no length. Before its delay it fills
//! backwards, after it it fills forwards with the value its last iteration ends on, and it reports
//! its start and its end once.

use zgui_dom::NodeIndex;
use zgui_style::{AnimationTime, Lifecycle};

use crate::support::{Harness, width};

/// How long one frame is, in seconds.
const FRAME: f64 = 0.016;

/// A sheet with one animation that moves `width` from 10px through 20px to 30px.
const SHEET: &str = "root { display: block; --tone: rgb(1, 1, 1) }
                     box { display: block; width: 50px; color: var(--tone) }
                     .forwards { animation: grow 0s linear forwards }
                     .none { animation: grow 0s linear }
                     .backwards { animation: grow 0s linear 125ms backwards }
                     .half { animation: grow 0s linear 0.5 forwards }
                     .alternate { animation: grow 0s linear 2 alternate forwards }
                     .slow { animation: grow 1s linear forwards }
                     .slow.instant { animation-duration: 0s }
                     .busy { border-top-left-radius: 2px }
                     .read { animation: settle 0s linear forwards }
                     @keyframes settle {
                         to { width: var(--rest, 30px) }
                     }
                     @keyframes grow {
                         from { width: 10px }
                         to { width: 30px }
                     }";

/// The same sheet with the theme's custom property changed.
fn dark() -> String {
    SHEET.replace("rgb(1, 1, 1)", "rgb(2, 2, 2)")
}

/// A styled document holding one element with `classes`, styled at time zero.
fn document(classes: &[&str]) -> (Harness, NodeIndex) {
    let mut harness = Harness::new();
    harness.add_author(SHEET);
    let node = harness.append(harness.root, "box");
    harness.set_classes(node, classes);
    harness.frame();
    harness.retire_all();
    (harness, node)
}

/// One frame at `now`, as the runtime runs it, and the edges its tick crossed.
fn frame_at(harness: &mut Harness, now: f64) -> Vec<(String, Lifecycle)> {
    let report = harness
        .engine
        .animation_tick(&harness.document, AnimationTime(now));
    for element in &report.elements {
        if element.properties.is_paint_only() || !(element.advancing || element.crossed) {
            continue;
        }
        harness
            .engine
            .mark_animation_restyle(&harness.document, element.index);
    }
    harness.engine.restyle(&mut harness.document, None);
    harness.retire_all();
    report
        .edges
        .iter()
        .map(|edge| (edge.name.clone(), edge.lifecycle))
        .collect()
}

/// The edges of every frame from `first` to `last`, together.
fn frames(harness: &mut Harness, first: u32, last: u32) -> Vec<(String, Lifecycle)> {
    (first..=last)
        .flat_map(|frame| frame_at(harness, f64::from(frame) * FRAME))
        .collect()
}

/// Started then ended, for the animation `grow`.
fn started_and_ended() -> Vec<(String, Lifecycle)> {
    vec![
        ("grow".to_owned(), Lifecycle::Started),
        ("grow".to_owned(), Lifecycle::Ended),
    ]
}

#[test]
fn a_filling_animation_of_no_length_holds_its_last_keyframe_from_the_first_frame() {
    let (mut harness, node) = document(&["forwards"]);
    assert_eq!(width(&harness, node), 30.0);
    assert_eq!(
        harness.engine.animations().running_elements(),
        [node],
        "the loop is asked back for the start and the end"
    );

    assert_eq!(frame_at(&mut harness, FRAME), started_and_ended());
    assert_eq!(frames(&mut harness, 2, 6), []);
    assert_eq!(width(&harness, node), 30.0);
}

#[test]
fn an_animation_of_no_length_without_a_fill_leaves_the_base_value() {
    let (mut harness, node) = document(&["none"]);
    assert_eq!(width(&harness, node), 50.0);
    assert_eq!(frame_at(&mut harness, FRAME), started_and_ended());
    assert_eq!(frames(&mut harness, 2, 4), []);
    assert_eq!(width(&harness, node), 50.0);
    assert!(harness.engine.animations().is_empty());
}

#[test]
fn a_delayed_animation_of_no_length_fills_backwards_until_its_start() {
    let (mut harness, node) = document(&["backwards"]);
    assert_eq!(width(&harness, node), 10.0);
    assert_eq!(frames(&mut harness, 1, 7), []);
    assert_eq!(width(&harness, node), 10.0);

    // Exactly at its start, where the engine's own progress divides zero by zero.
    assert_eq!(frame_at(&mut harness, 0.125), started_and_ended());
    assert_eq!(width(&harness, node), 50.0);
    assert_eq!(frames(&mut harness, 8, 12), []);
}

#[test]
fn a_fractional_iteration_count_ends_part_way() {
    let (harness, node) = document(&["half"]);
    assert_eq!(width(&harness, node), 20.0);
}

#[test]
fn an_even_alternating_count_ends_on_the_first_keyframe() {
    let (harness, node) = document(&["alternate"]);
    assert_eq!(width(&harness, node), 10.0);
}

#[test]
fn a_theme_swap_and_a_later_match_keep_the_value_and_report_nothing() {
    let (mut harness, node) = document(&["forwards"]);
    assert_eq!(frame_at(&mut harness, FRAME), started_and_ended());

    harness.replace(0, &dark());
    harness.frame();
    harness.retire_all();
    assert_eq!(width(&harness, node), 30.0, "after the theme swap");
    assert_eq!(frames(&mut harness, 2, 4), []);

    harness.set_classes(node, &["forwards", "busy"]);
    harness.frame();
    harness.retire_all();
    assert_eq!(width(&harness, node), 30.0, "after a later match");
    assert_eq!(frames(&mut harness, 5, 8), []);
}

#[test]
fn an_unfilled_animation_of_no_length_does_not_start_again_on_a_theme_swap() {
    let (mut harness, node) = document(&["none"]);
    assert_eq!(frame_at(&mut harness, FRAME), started_and_ended());

    harness.replace(0, &dark());
    harness.frame();
    harness.retire_all();
    assert_eq!(frames(&mut harness, 2, 4), []);
    assert_eq!(width(&harness, node), 50.0);
}

#[test]
fn a_running_animation_whose_duration_becomes_zero_ends_once() {
    let (mut harness, node) = document(&["slow"]);
    let crossed = frames(&mut harness, 1, 30);
    assert_eq!(crossed, [("grow".to_owned(), Lifecycle::Started)]);

    harness.set_classes(node, &["slow", "instant"]);
    harness.frame();
    harness.retire_all();
    let crossed = frames(&mut harness, 31, 36);
    assert_eq!(
        crossed,
        [("grow".to_owned(), Lifecycle::Ended)],
        "the animation ends once and is not cancelled"
    );
    assert_eq!(width(&harness, node), 30.0);
}

#[test]
fn a_filled_last_keyframe_of_no_length_takes_a_moved_custom_property() {
    let (mut harness, node) = document(&["read"]);
    assert_eq!(
        frame_at(&mut harness, FRAME),
        [
            ("settle".to_owned(), Lifecycle::Started),
            ("settle".to_owned(), Lifecycle::Ended),
        ]
    );
    assert_eq!(width(&harness, node), 30.0);

    let root = harness.root;
    harness.edit(|edit| {
        assert!(edit.set_custom_property(
            root,
            zgui_interned::CustomPropertyName::new("rest"),
            Some("40px")
        ));
    });
    let pass = harness.frame();
    harness.retire_all();
    assert_eq!(pass.refreshed, [node]);
    assert_eq!(width(&harness, node), 40.0);
    assert_eq!(frames(&mut harness, 2, 5), []);
    assert_eq!(width(&harness, node), 40.0);
}
