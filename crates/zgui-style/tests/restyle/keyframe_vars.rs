//! Keyframe values that read a custom property, as the property moves under a running animation.
//!
//! The engine computes keyframe values once, when it starts an animation. A positioner that learns
//! its side after the first layout sets the custom property its surface's entrance reads one frame
//! after the entrance started, and the entrance must take the new value from then on.

use style::values::computed::Transform;
use zgui_dom::NodeIndex;
use zgui_style::{AnimationTime, Lifecycle};

use crate::support::{Harness, animation_frame};

/// How long one frame is, in seconds.
const FRAME: f64 = 0.016;

/// An entrance whose first keyframe reads `--enter-y`, and a settle whose last one reads `--rest-y`.
const SHEET: &str = "root { display: block }
                     .pos { display: block }
                     .pos.bottom { --enter-y: -8px; --rest-y: 6px }
                     .pos.other { --unread: 1px }
                     .middle { display: block }
                     .enter { display: block; animation: enter 100ms linear both }
                     .enter.up { --enter-y: 8px }
                     .settle { display: block; animation: settle 100ms linear forwards }
                     .ref-a { display: block; transform: translate(0px, -4px) }
                     .ref-b { display: block; transform: translate(0px, 4px) }
                     .ref-c { display: block; transform: translate(0px, 0px) }
                     .ref-d { display: block; transform: translate(0px, 6px) }
                     @keyframes enter {
                         from { transform: translate(0px, var(--enter-y, 0px)) }
                         to { transform: translate(0px, 0px) }
                     }
                     @keyframes settle {
                         from { transform: translate(0px, 0px) }
                         to { transform: translate(0px, var(--rest-y, 0px)) }
                     }";

/// The computed transform of `node`.
fn transform(harness: &Harness, node: NodeIndex) -> Transform {
    harness
        .document
        .node(node)
        .primary_style()
        .expect("the element is styled")
        .get_box()
        .clone_transform()
}

/// A styled document: a positioner, a plain element in it, and an element of `class` in that.
///
/// Also holds one reference element per expected transform, in the order `a`, `b`, `c`, `d`.
fn document(class: &str) -> (Harness, NodeIndex, NodeIndex, [NodeIndex; 4]) {
    let mut harness = Harness::new();
    harness.add_author(SHEET);
    let positioner = harness.append(harness.root, "box");
    harness.set_classes(positioner, &["pos"]);
    let middle = harness.append(positioner, "box");
    harness.set_classes(middle, &["middle"]);
    let animated = harness.append(middle, "box");
    harness.set_classes(animated, &[class]);
    let references = ["ref-a", "ref-b", "ref-c", "ref-d"].map(|class| {
        let node = harness.append(harness.root, "box");
        harness.set_classes(node, &[class]);
        node
    });
    harness.frame();
    harness.retire_all();
    (harness, positioner, animated, references)
}

#[test]
fn an_ancestor_custom_property_reaches_a_running_entrance() {
    let (mut harness, positioner, animated, [half_up, ..]) = document("enter");
    animation_frame(&mut harness, FRAME);

    harness.set_classes(positioner, &["pos", "bottom"]);
    let pass = animation_frame(&mut harness, 2.0 * FRAME);
    assert_eq!(
        pass.refreshed,
        [animated],
        "the entrance reads the moved property only in its keyframes, two levels down"
    );

    animation_frame(&mut harness, 0.05);
    assert_eq!(
        transform(&harness, animated),
        transform(&harness, half_up),
        "half-way through, the entrance stands half-way from the new first keyframe"
    );
}

#[test]
fn an_own_custom_property_reaches_a_running_entrance() {
    let (mut harness, _positioner, animated, [_, half_down, ..]) = document("enter");
    animation_frame(&mut harness, FRAME);

    harness.set_classes(animated, &["enter", "up"]);
    animation_frame(&mut harness, 2.0 * FRAME);
    animation_frame(&mut harness, 0.05);
    assert_eq!(
        transform(&harness, animated),
        transform(&harness, half_down)
    );
}

#[test]
fn a_filled_last_keyframe_takes_the_new_value_without_a_second_end() {
    let (mut harness, positioner, animated, [_, _, rest, moved]) = document("settle");
    let mut crossed = Vec::new();
    for frame in 1..=10u32 {
        let now = f64::from(frame) * FRAME;
        let report = harness
            .engine
            .animation_tick(&harness.document, AnimationTime(now));
        crossed.extend(report.edges.iter().map(|edge| edge.lifecycle));
        animation_frame(&mut harness, now);
    }
    assert_eq!(crossed, [Lifecycle::Started, Lifecycle::Ended]);
    assert_eq!(transform(&harness, animated), transform(&harness, rest));

    harness.set_classes(positioner, &["pos", "bottom"]);
    let pass = harness.frame();
    harness.retire_all();
    assert_eq!(
        pass.refreshed,
        [animated],
        "the frame loop is told to tick the filled animation again"
    );
    assert_eq!(
        transform(&harness, animated),
        transform(&harness, moved),
        "the fill holds the last keyframe with the new value"
    );

    for frame in 11..=14u32 {
        let now = f64::from(frame) * FRAME;
        let report = harness
            .engine
            .animation_tick(&harness.document, AnimationTime(now));
        assert!(
            report.edges.is_empty(),
            "the new keyframes made the animation cross an edge again: {:?}",
            report.edges
        );
        assert!(report.elements.iter().all(|element| !element.advancing));
        animation_frame(&mut harness, now);
        assert_eq!(transform(&harness, animated), transform(&harness, moved));
    }
}

#[test]
fn a_property_no_keyframe_reads_leaves_the_animation_alone() {
    let (mut harness, positioner, animated, [.., rest, _]) = document("enter");
    animation_frame(&mut harness, FRAME);

    harness.set_classes(positioner, &["pos", "other"]);
    let pass = animation_frame(&mut harness, 2.0 * FRAME);
    assert!(pass.refreshed.is_empty());

    for frame in 3..=8u32 {
        animation_frame(&mut harness, f64::from(frame) * FRAME);
    }
    assert_eq!(transform(&harness, animated), transform(&harness, rest));
}
