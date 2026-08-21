//! Planning a `backdrop-filter`.

use zgui_geom::{Device, Rect};

use crate::frame::build::PlanBuilder;
use crate::frame::segment::EncoderOp;
use crate::frame::target::TargetRef;

/// Copies `region` of `beneath` into `into`.
///
/// The capture is a copy rather than a read, because a fragment shader cannot read the attachment
/// it is writing.
pub fn capture(
    builder: &mut PlanBuilder<'_>,
    beneath: TargetRef,
    into: TargetRef,
    region: Rect<i32, Device>,
) {
    if region.is_empty() {
        return;
    }
    builder.encoder(EncoderOp::Capture {
        source: beneath,
        destination: into,
        region,
    });
}

/// Lends a target to copy what lies beneath into, matching what it is copied from.
///
/// Returns `None` when the pool could not lend one, in which case the region is left as it is: an
/// unfiltered backdrop is the content it was meant to frost, which is a visible degradation and
/// not a wrong picture.
pub fn scratch(builder: &mut PlanBuilder<'_>, beneath: TargetRef) -> Option<TargetRef> {
    Some(TargetRef::Pool(builder.acquire_like(beneath)?))
}

/// Whether the copy for a backdrop over `beneath` can be the kept one.
///
/// The kept copy is allocated like the composed target and holds what it holds, and a copy between
/// two textures requires them to agree — so a backdrop *inside* a group, whose composite so far is
/// that group's own half-float target, takes a scratch copy of that instead. The damage was grown
/// on the understanding that the kept copy would be used, so a frame that finds otherwise here
/// says so and the one after it redraws everything.
pub fn keepable(beneath: TargetRef) -> bool {
    beneath == TargetRef::Composed
}
