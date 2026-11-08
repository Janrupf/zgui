//! Keyframe animations with a duration of `0s`.
//!
//! The engine starts no animation of no length, and cancels a running one whose duration becomes
//! `0s`. CSS gives such an animation an active interval of no length: before its delay it fills
//! backwards, after it the animation fills forwards with the value its last iteration ends on, and
//! it reports both its start and its end. A theme with its motion turned off sets every duration
//! to `0s`, and its surfaces must still take the value their entrance ends on.
//!
//! So after the engine has processed an element, an animation of no length that it skipped or
//! cancelled is built here and put into the table, and the element is cascaded again with it.
//! The tick ends it at its start time.

use servo_arc::Arc as ServoArc;
use style::Atom;
use style::animation::{
    Animation, AnimationSetKey, AnimationState, DocumentAnimationSet, ElementAnimationSet,
    KeyframesIterationState, maybe_start_animations,
};
use style::context::{CascadeInputs, StyleContext};
use style::data::ElementData;
use style::matching::MatchMethods;
use style::properties::ComputedValues;
use style::properties::longhands::animation_direction::computed_value::single_value::T as Direction;
use style::rule_tree::{CascadeLevel, CascadeOrigin, RuleCascadeFlags};
use style::style_resolver::{PseudoElementResolution, StyleResolverForElement};
use style::stylesheets::layer_rule::LayerOrder;
use style::stylist::RuleInclusion;
use style::values::computed::{AnimationDuration, Time};
use zgui_dom::Node;

use crate::driver::animations::refresh::unanimated;
use crate::driver::animations::set::key_of;

/// Whether `animation` has ended at `now`.
///
/// The engine's own answer divides by the duration, and an animation of no length then reads as
/// never having ended at its own start time.
pub(crate) fn ended(animation: &Animation, now: f64) -> bool {
    if animation.duration == 0.0 {
        return animation.state == AnimationState::Finished || now >= animation.started_at;
    }
    animation.has_ended(now)
}

/// Whether `style` names an animation of no length.
fn names_one(style: &ComputedValues) -> bool {
    let ui = style.get_ui();
    ui.specifies_animations()
        && ui.animation_name_iter().enumerate().any(|(index, name)| {
            !name.is_none() && ui.animation_duration_mod(index).seconds() == 0.0
        })
}

/// The names of the animations of no length `style` names.
fn names(style: &ComputedValues) -> Vec<Atom> {
    let ui = style.get_ui();
    ui.animation_name_iter()
        .enumerate()
        .filter(|(index, _)| ui.animation_duration_mod(*index).seconds() == 0.0)
        .filter_map(|(_, name)| name.as_atom().cloned())
        .collect()
}

/// Whether `name` starts to apply to the element, which is when an animation of that name starts.
///
/// A restyle that keeps the name, a theme change among them, starts nothing: an animation of no
/// length that the tick dropped after its end stays dropped.
fn newly_named(old: Option<&ComputedValues>, name: &Atom) -> bool {
    let Some(old) = old else {
        return true;
    };
    old.clone_display().is_none()
        || !old
            .get_ui()
            .animation_name_iter()
            .any(|named| named.as_atom() == Some(name))
}

/// `style` with every duration of `0s` made one second long, so the engine builds those animations.
pub(crate) fn with_placeholder_durations(
    style: &ServoArc<ComputedValues>,
) -> ServoArc<ComputedValues> {
    if !names_one(style) {
        return style.clone();
    }
    let mut style = style.clone();
    let ui = ServoArc::make_mut(&mut style).mutate_ui();
    let mut durations = ui.clone_animation_duration();
    for duration in durations.0.iter_mut() {
        if duration.seconds() == 0.0 {
            *duration = AnimationDuration::Time(Time::from_seconds(1.0));
        }
    }
    ui.set_animation_duration(durations);
    style
}

/// The direction an animation running in `direction` plays its iteration `index` in.
fn direction_at(direction: Direction, index: f64) -> Direction {
    let even = index % 2.0 == 0.0;
    match direction {
        Direction::Normal => Direction::Normal,
        Direction::Reverse => Direction::Reverse,
        Direction::Alternate if even => Direction::Normal,
        Direction::Alternate => Direction::Reverse,
        Direction::AlternateReverse if even => Direction::Reverse,
        Direction::AlternateReverse => Direction::Normal,
    }
}

/// The iteration count of `animation`.
fn count(animation: &Animation) -> f64 {
    match animation.iteration_state {
        KeyframesIterationState::Finite(_, count) => count,
        KeyframesIterationState::Infinite(_) => f64::INFINITY,
    }
}

/// Puts an animation of no length into the iteration and direction its before phase reads.
fn before_phase(animation: &mut Animation) {
    let count = count(animation);
    animation.iteration_state = if count.is_infinite() {
        KeyframesIterationState::Infinite(0.0)
    } else {
        KeyframesIterationState::Finite(0.0, count)
    };
    animation.current_direction = direction_at(animation.direction, 0.0);
}

/// Puts an animation of no length into the iteration and direction its after phase reads.
///
/// The engine evaluates a finished animation at the end of its current iteration, so the current
/// iteration is the last one and holds the fraction of it the iteration count leaves. An infinite
/// count ends a whole iteration forwards.
pub(crate) fn after_phase(animation: &mut Animation) {
    let count = count(animation);
    if count.is_infinite() {
        animation.iteration_state = KeyframesIterationState::Finite(0.0, 1.0);
        animation.current_direction = match animation.direction {
            Direction::Reverse => Direction::Reverse,
            _ => Direction::Normal,
        };
        return;
    }
    let last = if count > 0.0 { count.ceil() - 1.0 } else { 0.0 };
    animation.iteration_state = KeyframesIterationState::Finite(last, count);
    animation.current_direction = direction_at(animation.direction, last);
}

/// The time the active interval of `animation` starts at, after its delay.
fn start_of(animation: &Animation) -> f64 {
    let done = match animation.iteration_state {
        KeyframesIterationState::Finite(current, _)
        | KeyframesIterationState::Infinite(current) => current,
    };
    animation.started_at - done * animation.duration
}

/// Whether `animation`, as it stood before the engine cancelled it, had ended at `now`.
fn had_ended(animation: &Animation, now: f64) -> bool {
    if animation.duration == 0.0 {
        return now >= animation.started_at;
    }
    let mut running = animation.clone();
    running.state = AnimationState::Running;
    running.has_ended(now)
}

/// Gives the element its animations of no length after the engine processed its new style, and
/// returns the style it replaced for them, if it did.
///
/// An animation the engine cancelled because its duration became `0s` goes back into the table
/// with the timing it had: finished when it had ended or its start has passed, so the tick ends it
/// once, and pending otherwise. One the engine skipped is added when its name starts to apply,
/// finished when its start has passed so the tick reports its start and end, and pending
/// otherwise. The element is then cascaded again with its animation values.
pub(crate) fn reconcile<'doc>(
    context: &mut StyleContext<Node<'doc>>,
    element: Node<'doc>,
    data: &mut ElementData,
    old: Option<&ServoArc<ComputedValues>>,
) -> Option<ServoArc<ComputedValues>> {
    let new = data.styles.get_primary()?;
    if !names_one(new) || new.clone_display().is_none() {
        return None;
    }
    let new = new.clone();
    let shared = context.shared;
    let now = shared.current_time_for_animations;
    let key = key_of(element.index());

    let wanted: Vec<Atom> = {
        let sets = shared.animations.sets.read();
        let set = sets.get(&key);
        names(&new)
            .into_iter()
            .filter(|name| {
                let (mut live, mut cancelled) = (false, false);
                for held in set.into_iter().flat_map(|set| set.animations.iter()) {
                    if held.name == *name {
                        cancelled |= held.state == AnimationState::Canceled;
                        live |= held.state != AnimationState::Canceled;
                    }
                }
                !live && (cancelled || newly_named(old.map(|old| &**old), name))
            })
            .collect()
    };
    if wanted.is_empty() {
        return None;
    }

    let built = build(context, element, &new, &wanted);
    if built.is_empty() {
        return None;
    }
    put(&shared.animations, &key, built, now);

    let restyled = cascade_animations(context, element, &new, &key)?;
    if let Some(old) = old {
        data.damage |= element
            .compute_style_difference(old, &restyled, None)
            .damage;
    }
    data.styles.primary.replace(restyled)
}

/// Fresh animations of no length for `names`, built on `style` without its animation values.
fn build<'doc>(
    context: &mut StyleContext<Node<'doc>>,
    element: Node<'doc>,
    style: &ServoArc<ComputedValues>,
    names: &[Atom],
) -> Vec<Animation> {
    let base = with_placeholder_durations(&unanimated(context, element, style));
    let shared = context.shared;
    let mut fresh = ElementAnimationSet::default();
    let mut resolver = StyleResolverForElement::new(
        element,
        context,
        RuleInclusion::All,
        PseudoElementResolution::IfApplicable,
    );
    maybe_start_animations(element, shared, &base, &mut fresh, &mut resolver);
    let now = shared.current_time_for_animations;
    fresh
        .animations
        .into_iter()
        .filter(|animation| names.contains(&animation.name))
        .map(|mut animation| {
            animation.duration = 0.0;
            animation.started_at = now + animation.delay;
            animation
        })
        .collect()
}

/// Puts the built animations into the element's row, each in the state its timing gives it.
fn put(table: &DocumentAnimationSet, key: &AnimationSetKey, built: Vec<Animation>, now: f64) {
    let mut sets = table.sets.write();
    let set = sets.entry(key.clone()).or_default();
    for mut animation in built {
        let cancelled = set
            .animations
            .iter_mut()
            .find(|held| held.name == animation.name && held.state == AnimationState::Canceled);
        match cancelled {
            Some(held) => {
                // The animation keeps its own start and its reported edges.
                animation.started_at = start_of(held);
                animation.is_new = held.is_new;
                animation.state = if had_ended(held, now) {
                    AnimationState::Finished
                } else if now >= animation.started_at {
                    AnimationState::Running
                } else {
                    AnimationState::Pending
                };
                place(&mut animation);
                *held = animation;
            }
            None => {
                animation.is_new = true;
                animation.state = if now >= animation.started_at {
                    AnimationState::Finished
                } else {
                    AnimationState::Pending
                };
                place(&mut animation);
                set.animations.push(animation);
            }
        }
    }
}

/// Gives an animation of no length the iteration and direction of the phase its state is in.
fn place(animation: &mut Animation) {
    if animation.state == AnimationState::Pending {
        before_phase(animation);
    } else {
        after_phase(animation);
    }
}

/// `style` cascaded again with the element's current animation and transition values, or nothing
/// when those values are already the ones it holds.
fn cascade_animations<'doc>(
    context: &mut StyleContext<Node<'doc>>,
    element: Node<'doc>,
    style: &ServoArc<ComputedValues>,
    key: &AnimationSetKey,
) -> Option<ServoArc<ComputedValues>> {
    let shared = context.shared;
    let declarations = shared.animations.get_all_declarations(
        key,
        shared.current_time_for_animations,
        element.store().lock(),
    );
    let mut rules = style.rules().clone();
    for (origin, block) in [
        (CascadeOrigin::Transitions, &declarations.transitions),
        (CascadeOrigin::Animations, &declarations.animations),
    ] {
        let mut important_rules_changed = false;
        if let Some(replaced) = shared.stylist.rule_tree().update_rule_at_level(
            CascadeLevel::new(origin),
            LayerOrder::root(),
            block.as_ref().map(|block| block.borrow_arc()),
            &rules,
            &shared.guards,
            &mut important_rules_changed,
        ) {
            rules = replaced;
        }
    }
    if rules == *style.rules() {
        return None;
    }
    let inputs = CascadeInputs {
        rules: Some(rules),
        visited_rules: style.visited_rules().cloned(),
        flags: style.flags.for_cascade_inputs(),
        included_cascade_flags: RuleCascadeFlags::empty(),
    };
    Some(
        StyleResolverForElement::new(
            element,
            context,
            RuleInclusion::All,
            PseudoElementResolution::IfApplicable,
        )
        .cascade_style_and_visited_with_default_parents(inputs)
        .0,
    )
}

#[cfg(test)]
mod tests {
    use super::direction_at;
    use style::properties::longhands::animation_direction::computed_value::single_value::T as Direction;

    #[test]
    fn an_alternating_animation_plays_its_odd_iterations_backwards() {
        assert_eq!(direction_at(Direction::Alternate, 0.0), Direction::Normal);
        assert_eq!(direction_at(Direction::Alternate, 1.0), Direction::Reverse);
        assert_eq!(
            direction_at(Direction::AlternateReverse, 0.0),
            Direction::Reverse
        );
        assert_eq!(
            direction_at(Direction::AlternateReverse, 1.0),
            Direction::Normal
        );
        assert_eq!(direction_at(Direction::Reverse, 1.0), Direction::Reverse);
    }
}
