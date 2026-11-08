//! Keyframe values computed again when a custom property they read moves.
//!
//! The engine computes the values of an animation's keyframes once, when it starts the animation,
//! against the custom properties the element holds at that moment. It starts the animation again
//! only when the animation declarations change. A keyframe that reads `var(--x)` therefore keeps
//! the value `--x` had at the start, and a change to `--x` reaches the base style and stops there.
//!
//! So an element whose custom properties moved while its animation declarations stayed builds
//! fresh animations from the new style, and each running or filling animation that reads a moved
//! name takes the fresh keyframes and keeps its own timing. The engine's own processing of the
//! element, which follows, sees the changed set and cascades the new values.

use servo_arc::Arc as ServoArc;
use style::Atom;
use style::animation::{AnimationState, ElementAnimationSet, maybe_start_animations};
use style::context::{CascadeInputs, StyleContext};
use style::custom_properties::{ComputedCustomProperties, Name};
use style::dom::TElement;
use style::properties::ComputedValues;
use style::rule_tree::{CascadeLevel, CascadeOrigin, RuleCascadeFlags};
use style::style_resolver::{PseudoElementResolution, StyleResolverForElement};
use style::stylesheets::layer_rule::LayerOrder;
use style::stylist::RuleInclusion;
use zgui_dom::Node;
use zgui_profile::{Counter, counter};

use crate::driver::animations::set::key_of;
use crate::driver::animations::zero;
use crate::driver::traversal::RecalcStyle;

/// Gives the element's animations new keyframe values when a custom property they read moved
/// between `old` and `new`, and reports whether any animation took them.
///
/// Runs before the engine processes the element's animations for `new`. Does nothing when the
/// animation declarations changed as well, because the engine then starts the animations again
/// itself.
pub(crate) fn keyframes<'doc>(
    traversal: &RecalcStyle<'_>,
    context: &mut StyleContext<Node<'doc>>,
    element: Node<'doc>,
    old: &ComputedValues,
    new: &ServoArc<ComputedValues>,
) -> bool {
    let ui = new.get_ui();
    if !ui.specifies_animations() {
        return false;
    }
    let (before, after) = (old.custom_properties(), new.custom_properties());
    if before.inherited == after.inherited && before.non_inherited == after.non_inherited {
        return false;
    }
    if !old.get_ui().animations_equals(ui)
        || old.clone_display().is_none()
        || new.clone_display().is_none()
        || !element.has_css_animations(context.shared, None)
    {
        return false;
    }
    let moved: Vec<Atom> = ui
        .animation_name_iter()
        .filter_map(|name| name.as_atom().cloned())
        .filter(|name| {
            traversal
                .keyframe_refs_of(element, name)
                .is_some_and(|refs| refs.reads.iter().any(|read| differs(before, after, read)))
        })
        .collect();
    if moved.is_empty() {
        return false;
    }

    let key = key_of(element.index());
    let shared = context.shared;
    let held = shared.animations.sets.read().get(&key).is_some_and(|set| {
        set.animations.iter().any(|animation| {
            animation.state != AnimationState::Canceled && moved.contains(&animation.name)
        })
    });
    if !held {
        return false;
    }

    // The keyframes take every property they leave out from the style they are built on, so that
    // style is the element's own without the values its animations write. An animation of no
    // length is built too, as one second long.
    let base = zero::with_placeholder_durations(&unanimated(context, element, new));
    let mut fresh = ElementAnimationSet::default();
    let mut resolver = StyleResolverForElement::new(
        element,
        context,
        RuleInclusion::All,
        PseudoElementResolution::IfApplicable,
    );
    maybe_start_animations(element, shared, &base, &mut fresh, &mut resolver);

    let mut sets = shared.animations.sets.write();
    let Some(set) = sets.get_mut(&key) else {
        return false;
    };
    let mut refreshed = false;
    for animation in &mut set.animations {
        if animation.state == AnimationState::Canceled || !moved.contains(&animation.name) {
            continue;
        }
        let Some(fresh) = fresh.animations.iter().find(|a| a.name == animation.name) else {
            continue;
        };
        // The timing is the animation's own: only the keyframes are new.
        let mut replacement = fresh.clone();
        replacement.started_at = animation.started_at;
        replacement.duration = animation.duration;
        replacement.delay = animation.delay;
        replacement.fill_mode = animation.fill_mode;
        replacement.iteration_state = animation.iteration_state.clone();
        replacement.state = animation.state.clone();
        replacement.direction = animation.direction;
        replacement.current_direction = animation.current_direction;
        replacement.is_new = animation.is_new;
        *animation = replacement;
        refreshed = true;
    }
    if refreshed {
        set.dirty = true;
        counter::bump(Counter::KeyframesRefreshed);
    }
    refreshed
}

/// Whether the custom property `name`, without its `--` prefix, differs between the two maps.
fn differs(
    before: &ComputedCustomProperties,
    after: &ComputedCustomProperties,
    name: &str,
) -> bool {
    let name = Name::from(name);
    before.inherited.get(&name) != after.inherited.get(&name)
        || before.non_inherited.get(&name) != after.non_inherited.get(&name)
}

/// `style` without the declarations its animations contribute.
pub(crate) fn unanimated<'doc>(
    context: &mut StyleContext<Node<'doc>>,
    element: Node<'doc>,
    style: &ServoArc<ComputedValues>,
) -> ServoArc<ComputedValues> {
    let shared = context.shared;
    let mut important_rules_changed = false;
    let Some(rules) = shared.stylist.rule_tree().update_rule_at_level(
        CascadeLevel::new(CascadeOrigin::Animations),
        LayerOrder::root(),
        None,
        style.rules(),
        &shared.guards,
        &mut important_rules_changed,
    ) else {
        return style.clone();
    };
    if rules == *style.rules() {
        return style.clone();
    }
    let inputs = CascadeInputs {
        rules: Some(rules),
        visited_rules: style.visited_rules().cloned(),
        flags: style.flags.for_cascade_inputs(),
        included_cascade_flags: RuleCascadeFlags::empty(),
    };
    StyleResolverForElement::new(
        element,
        context,
        RuleInclusion::All,
        PseudoElementResolution::IfApplicable,
    )
    .cascade_style_and_visited_with_default_parents(inputs)
    .0
}
