//! Which slide is showing, which slides there are, and where each of them sits.

use zgui::geom::{Device, DevicePx, Rect};
use zgui::prelude::*;
use zgui::reactive::{LocalStorage, Memo, RwSignal};
use zgui_ui_primitives::{Controllable, Orientation};

/// One slide, as the carousel knows it.
///
/// The name is handed out once and never changes; the geometry is asked afresh every frame it
/// moves, because a slide is only as wide as its own content and a carousel that assumed every
/// slide was the width of its viewport would step past a narrow one and stop short of a wide one.
#[derive(Copy, Clone)]
pub struct CarouselSlot {
    /// What the carousel calls this slide.
    id: u64,
    /// Its box in the window, as of the last completed layout.
    geometry: Signal<Option<Rect<DevicePx, Device>>, LocalStorage>,
}

impl CarouselSlot {
    /// What the carousel calls this slide.
    #[must_use]
    pub fn id(self) -> u64 {
        self.id
    }

    /// Where this slide's box is in the window, along `orientation`, as its leading and trailing
    /// edges. `None` until the slide has been laid out.
    fn span(self, orientation: Orientation) -> Option<(f32, f32)> {
        let box_ = self.geometry.get()?;
        Some(along(box_, orientation))
    }

    /// Where this slide's leading edge is in the window, along `orientation`.
    ///
    /// In the window rather than in the track, because that is the space geometry is observed in
    /// here. What anything does with it is a *difference* between two readings taken this way —
    /// which is the same number whether or not the track it is on has been translated, since a
    /// translation moves a track and its slides together.
    ///
    /// `None` until the slide has been laid out, which is every frame before the first.
    #[must_use]
    pub fn start(self, orientation: Orientation) -> Option<f32> {
        let box_ = self.geometry.get()?;
        Some(match orientation {
            Orientation::Vertical => box_.origin.y.0,
            _ => box_.origin.x.0,
        })
    }
}

/// The leading and trailing edges of `box_` along `orientation`.
fn along(box_: Rect<DevicePx, Device>, orientation: Orientation) -> (f32, f32) {
    match orientation {
        Orientation::Vertical => (box_.origin.y.0, box_.origin.y.0 + box_.size.height.0),
        _ => (box_.origin.x.0, box_.origin.x.0 + box_.size.width.0),
    }
}

/// How far a track can travel, and the last slide a step can show.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Reach {
    /// How far the track can travel before the last slide leaves the trailing edge, in device
    /// pixels.
    pub travel: f32,
    /// The last slide a step can show, counted from zero.
    pub last: usize,
}

/// How far a track `track` long can travel along slides that start `starts` from its own leading
/// edge and end `end` from it, and the last slide a step can show.
///
/// The last slide a step shows is the first one whose start is at or past the full travel, so
/// the last slide sits against the trailing edge and the viewport never shows empty space after
/// it. When every slide fits, the first slide is the last one a step shows.
///
/// ```
/// use zgui_ui::carousel::reach;
///
/// // Five slides of a third of a 300px track each.
/// let starts = [0.0, 100.0, 200.0, 300.0, 400.0];
/// let reach = reach(&starts, 500.0, 300.0);
/// assert_eq!((reach.travel, reach.last), (200.0, 2));
/// ```
#[must_use]
pub fn reach(starts: &[f32], end: f32, track: f32) -> Reach {
    let travel = (end - track).max(0.0);
    let last = starts
        .iter()
        .position(|start| *start >= travel - 0.5)
        .unwrap_or_else(|| starts.len().saturating_sub(1));
    Reach { travel, last }
}

/// What the slides and the arrows read to know where the carousel is.
///
/// `Copy`, so a handler stores one without cloning, and reachable from any depth with
/// [`CarouselContext::current`].
#[derive(Copy, Clone)]
pub struct CarouselContext {
    /// Which slide is showing, counted from zero.
    index: Controllable<usize>,
    /// One slot per slide, in the order they were written.
    slots: RwSignal<Vec<CarouselSlot>, LocalStorage>,
    /// The next name to hand out.
    next: RwSignal<u64, LocalStorage>,
    /// Which way the slides run.
    orientation: Orientation,
    /// Whether stepping past the last slide goes back to the first.
    wrap: bool,
    /// The track the slides sit on.
    track: NodeRef,
    /// The track's box in the window, as of the last completed layout.
    track_box: Signal<Option<Rect<DevicePx, Device>>, LocalStorage>,
    /// How far the track can travel and the last slide a step shows. `None` before layout.
    reach: Memo<Option<Reach>>,
}

impl CarouselContext {
    /// Wires a carousel up from what it was told about its own index.
    #[must_use]
    pub fn new(index: Controllable<usize>, orientation: Orientation, wrap: bool) -> Self {
        let slots: RwSignal<Vec<CarouselSlot>, LocalStorage> = RwSignal::new_local(Vec::new());
        let track = NodeRef::new();
        let measured = track.observe_border_box();
        // Recomputed whenever a slide or the track moves, which is every frame of a step. The
        // answer only changes when the sizes do, so nothing downstream reruns with it.
        let reach = Memo::new(move |_| {
            let (from, to) = along(measured.get()?, orientation);
            slots.with(|slots| {
                let mut starts = Vec::with_capacity(slots.len());
                let mut end = 0.0_f32;
                for slot in slots {
                    let (start, stop) = slot.span(orientation)?;
                    starts.push(start - from);
                    end = end.max(stop - from);
                }
                Some(reach(&starts, end, to - from))
            })
        });
        Self {
            index,
            slots,
            next: RwSignal::new_local(1),
            orientation,
            wrap,
            track,
            track_box: measured,
            reach,
        }
    }

    /// Publishes this to every scope below the current one, and hands it back.
    pub fn provide(self) -> Self {
        provide_local_context(self);
        self
    }

    /// The carousel the calling scope is inside, when it is inside one.
    #[must_use]
    pub fn current() -> Option<Self> {
        use_local_context::<Self>()
    }

    /// Which slide is showing, counted from zero.
    #[must_use]
    pub fn index(self) -> usize {
        self.index.get()
    }

    /// How many slides there are.
    #[must_use]
    pub fn count(self) -> usize {
        self.slots.with(Vec::len)
    }

    /// Which way the slides run.
    #[must_use]
    pub fn orientation(self) -> Orientation {
        self.orientation
    }

    /// The track the slides sit on, which the carousel measures.
    #[must_use]
    pub fn track(self) -> NodeRef {
        self.track
    }

    /// The track's box in the window, as of the last completed layout.
    #[must_use]
    pub fn track_box(self) -> Signal<Option<Rect<DevicePx, Device>>, LocalStorage> {
        self.track_box
    }

    /// How far the track can travel and the last slide a step shows, once laid out.
    #[must_use]
    pub fn reach(self) -> Option<Reach> {
        self.reach.get()
    }

    /// The last slide a step can show: the one that puts the last slide against the trailing edge.
    /// Before layout, the last slide.
    #[must_use]
    pub fn last_index(self) -> usize {
        self.reach
            .get()
            .map_or_else(|| self.count().saturating_sub(1), |reach| reach.last)
    }

    /// Whether there is a slide before this one to go to.
    #[must_use]
    pub fn has_previous(self) -> bool {
        self.wrap || self.index().min(self.last_index()) > 0
    }

    /// Whether there is a slide after this one to go to.
    #[must_use]
    pub fn has_next(self) -> bool {
        self.wrap || (self.count() > 0 && self.index() < self.last_index())
    }

    /// Shows the slide before this one, wrapping when the carousel was told to.
    pub fn previous(self) {
        if self.count() == 0 {
            return;
        }
        let last = self.reach.get_untracked().map_or_else(
            || self.slots.with_untracked(Vec::len).saturating_sub(1),
            |reach| reach.last,
        );
        let at = self.index.get_untracked().min(last);
        if at > 0 {
            self.index.set(at - 1);
        } else if self.wrap {
            self.index.set(last);
        }
    }

    /// Shows the slide after this one, wrapping when the carousel was told to.
    pub fn next(self) {
        if self.count() == 0 {
            return;
        }
        let last = self.reach.get_untracked().map_or_else(
            || self.slots.with_untracked(Vec::len).saturating_sub(1),
            |reach| reach.last,
        );
        let at = self.index.get_untracked().min(last);
        if at < last {
            self.index.set(at + 1);
        } else if self.wrap {
            self.index.set(0);
        }
    }

    /// Adds a slide at the end, and takes it out again when its scope goes away.
    ///
    /// The handle is the slide's own name; where it *is* is [`CarouselContext::position_of`],
    /// which is asked afresh every time, because a slide's position moves when anything before it
    /// comes or goes and a number captured once would be wrong from then on.
    ///
    /// `element` is watched from here on, because how far the track has to travel to bring this
    /// slide to the front is a question about where the slide actually ended up rather than about
    /// how many slides there are.
    #[must_use]
    pub fn register(self, element: NodeRef) -> u64 {
        let id = self.next.get_untracked();
        self.next.set(id + 1);
        let slot = CarouselSlot {
            id,
            geometry: element.observe_border_box(),
        };
        self.slots.update(|slots| slots.push(slot));
        on_cleanup_local(move || {
            self.slots
                .try_update(|slots| slots.retain(|slot| slot.id() != id));
        });
        id
    }

    /// Where the slide called `id` sits, counted from zero.
    #[must_use]
    pub fn position_of(self, id: u64) -> Option<usize> {
        self.slots
            .with(|slots| slots.iter().position(|slot| slot.id() == id))
    }

    /// Where the showing slide's leading edge is in the window, along the carousel's own axis.
    ///
    /// Taken against the track's own leading edge, this is the whole of the geometry: the track has
    /// to travel exactly that far for the showing slide to sit against the viewport's leading edge,
    /// whatever the slides are worth and however many of them a viewport holds at once.
    ///
    /// `None` before the slides have been laid out, and for an index no slide answers to.
    #[must_use]
    pub fn start_of_showing(self) -> Option<f32> {
        let at = self.index();
        let slot = self.slots.with(|slots| slots.get(at).copied())?;
        slot.start(self.orientation)
    }
}

#[cfg(test)]
mod tests {
    use super::reach;

    #[test]
    fn a_track_of_thirds_stops_where_the_last_slide_meets_the_trailing_edge() {
        let starts = [0.0, 112.0, 224.0, 336.0, 448.0];
        let found = reach(&starts, 560.0, 336.0);
        assert_eq!(found.travel, 224.0);
        assert_eq!(found.last, 2);
    }

    #[test]
    fn slides_that_all_fit_leave_nowhere_to_go() {
        let found = reach(&[0.0, 112.0, 224.0], 336.0, 336.0);
        assert_eq!((found.travel, found.last), (0.0, 0));
    }

    #[test]
    fn slides_as_wide_as_the_track_are_each_a_step() {
        let found = reach(&[0.0, 336.0, 672.0], 1008.0, 336.0);
        assert_eq!((found.travel, found.last), (672.0, 2));
    }

    #[test]
    fn slides_wider_than_the_track_keep_the_last_one_reachable() {
        let found = reach(&[0.0, 400.0, 640.0], 1040.0, 336.0);
        assert_eq!(
            found.last, 2,
            "the last slide starts short of the full travel"
        );
    }

    #[test]
    fn a_rounding_error_does_not_cost_a_step() {
        let found = reach(&[0.0, 112.0, 223.7, 336.0], 448.0, 224.0);
        assert_eq!(
            found.last, 2,
            "a start within half a pixel of the travel is the last"
        );
    }

    #[test]
    fn no_slides_is_no_travel() {
        let found = reach(&[], 0.0, 300.0);
        assert_eq!((found.travel, found.last), (0.0, 0));
    }
}
