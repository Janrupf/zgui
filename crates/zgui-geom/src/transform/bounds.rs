//! The axis-aligned box a transformed rectangle occupies.

use crate::rect::Rect;
use crate::space::Device;
use crate::transform::Matrix4;
use crate::unit::DevicePx;

/// The axis-aligned rectangle that contains `rect` after `matrix` is applied to it.
///
/// A transformed rectangle is not a rectangle, so what damage, culling and a spatial index all
/// need is the smallest axis-aligned box containing its four corners. Points behind the viewer
/// under a perspective matrix are dropped rather than projected, because their projection is a
/// reflection through the origin and would report ink on the wrong side of the screen; a rectangle
/// with no corner in front of the viewer occupies nothing at all.
///
/// ```
/// use zgui_geom::{Device, DevicePx, Matrix4, Point, Rect, Size, transformed_bounds};
///
/// let rect: Rect<DevicePx, Device> = Rect::new(
///     Point::new(DevicePx(0.0), DevicePx(0.0)),
///     Size::new(DevicePx(10.0), DevicePx(4.0)),
/// );
/// let moved = transformed_bounds(&Matrix4::translation(3.0, 5.0, 0.0), rect);
/// assert_eq!(moved.origin.x, DevicePx(3.0));
/// assert_eq!(moved.size.width, DevicePx(10.0));
/// ```
pub fn transformed_bounds(
    matrix: &Matrix4,
    rect: Rect<DevicePx, Device>,
) -> Rect<DevicePx, Device> {
    // A shift moves the box and leaves its shape alone, so the four corners are the two corners
    // plus the shift and the general path's arithmetic collapses to an addition. Reached by the
    // commonest transform there is — a box animating across the screen — and bit-for-bit what the
    // loop below produces: every product the loop forms is against a one or a zero, `w` comes out
    // as exactly one, and the reciprocal of one leaves a float alone.
    if let Some((by_x, by_y)) = matrix.as_translation() {
        let (left, right) = (rect.left().0 + by_x, rect.right().0 + by_x);
        let (top, bottom) = (rect.top().0 + by_y, rect.bottom().0 + by_y);
        return Rect::from_corners(
            crate::point::Point::new(DevicePx(left.min(right)), DevicePx(top.min(bottom))),
            crate::point::Point::new(DevicePx(left.max(right)), DevicePx(top.max(bottom))),
        );
    }
    let corners = [
        (rect.left().0, rect.top().0),
        (rect.right().0, rect.top().0),
        (rect.right().0, rect.bottom().0),
        (rect.left().0, rect.bottom().0),
    ];
    let mut min = (f32::INFINITY, f32::INFINITY);
    let mut max = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut seen = 0;
    for (x, y) in corners {
        let projected = matrix.transform_vector4([x, y, 0.0, 1.0]);
        if projected[3] <= 0.0 {
            continue;
        }
        let inverse_w = projected[3].recip();
        let (px, py) = (projected[0] * inverse_w, projected[1] * inverse_w);
        min = (min.0.min(px), min.1.min(py));
        max = (max.0.max(px), max.1.max(py));
        seen += 1;
    }
    if seen == 0 {
        return Rect::ZERO;
    }
    Rect::from_corners(
        crate::point::Point::new(DevicePx(min.0), DevicePx(min.1)),
        crate::point::Point::new(DevicePx(max.0), DevicePx(max.1)),
    )
}

#[cfg(test)]
mod tests {
    use super::transformed_bounds;
    use crate::point::Point;
    use crate::rect::Rect;
    use crate::space::Device;
    use crate::transform::Matrix4;
    use crate::unit::DevicePx;

    /// The general path, kept here so the shortcut can be held against it.
    fn by_corners(matrix: &Matrix4, rect: Rect<DevicePx, Device>) -> Rect<DevicePx, Device> {
        let corners = [
            (rect.left().0, rect.top().0),
            (rect.right().0, rect.top().0),
            (rect.right().0, rect.bottom().0),
            (rect.left().0, rect.bottom().0),
        ];
        let mut min = (f32::INFINITY, f32::INFINITY);
        let mut max = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for (x, y) in corners {
            let projected = matrix.transform_vector4([x, y, 0.0, 1.0]);
            let inverse_w = projected[3].recip();
            let (px, py) = (projected[0] * inverse_w, projected[1] * inverse_w);
            min = (min.0.min(px), min.1.min(py));
            max = (max.0.max(px), max.1.max(py));
        }
        Rect::from_corners(
            Point::new(DevicePx(min.0), DevicePx(min.1)),
            Point::new(DevicePx(max.0), DevicePx(max.1)),
        )
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect<DevicePx, Device> {
        Rect::new(
            Point::new(DevicePx(x), DevicePx(y)),
            crate::size::Size::new(DevicePx(w), DevicePx(h)),
        )
    }

    /// The shortcut is an optimisation and owes bit-for-bit agreement, rather than nearness: it
    /// feeds damage and culling, and a box that disagrees by one unit in the last place with the
    /// answer the general path would have given is a box that redraws differently.
    #[test]
    fn the_shortcut_for_a_shift_agrees_to_the_last_bit() {
        let awkward = [
            0.0, 1.0, -1.0, 0.1, -0.1, 1e-7, -1e-7, 1e7, -1e7, 1234.567_9, -0.000_003,
        ];
        for &by_x in &awkward {
            for &by_y in &awkward {
                let matrix = Matrix4::translation(by_x, by_y, 0.0);
                assert!(matrix.as_translation().is_some(), "{by_x} {by_y}");
                for &(x, y, w, h) in &[
                    (0.0, 0.0, 10.0, 4.0),
                    (-3.5, 7.25, 0.0, 0.0),
                    (1e6, -1e6, 0.333, 1e-6),
                    (-0.000_001, 0.000_001, 1e7, 1e7),
                ] {
                    let subject = rect(x, y, w, h);
                    let fast = transformed_bounds(&matrix, subject);
                    let slow = by_corners(&matrix, subject);
                    assert_eq!(
                        fast.origin.x.0.to_bits(),
                        slow.origin.x.0.to_bits(),
                        "x: {by_x} {by_y} {x} {y} {w} {h}",
                    );
                    assert_eq!(fast.origin.y.0.to_bits(), slow.origin.y.0.to_bits());
                    assert_eq!(fast.size.width.0.to_bits(), slow.size.width.0.to_bits());
                    assert_eq!(fast.size.height.0.to_bits(), slow.size.height.0.to_bits());
                }
            }
        }
    }

    /// Anything that is not purely a shift has to reach the general path, or it is answered by
    /// arithmetic that does not apply to it.
    #[test]
    fn only_a_shift_takes_the_shortcut() {
        assert!(Matrix4::IDENTITY.as_translation() == Some((0.0, 0.0)));
        assert!(Matrix4::translation(2.0, 3.0, 0.0).as_translation() == Some((2.0, 3.0)));
        assert!(Matrix4::scale(2.0, 2.0, 1.0).as_translation().is_none());
        assert!(Matrix4::perspective(400.0).as_translation().is_none());
        assert!(
            Matrix4::translation(1.0, 2.0, 3.0)
                .as_translation()
                .is_none()
        );
    }

    /// A scale still goes the long way round and still answers correctly.
    #[test]
    fn a_scale_is_bounded_by_its_transformed_corners() {
        let scaled = transformed_bounds(&Matrix4::scale(2.0, 3.0, 1.0), rect(1.0, 1.0, 4.0, 4.0));
        assert_eq!(scaled.origin.x, DevicePx(2.0));
        assert_eq!(scaled.origin.y, DevicePx(3.0));
        assert_eq!(scaled.size.width, DevicePx(8.0));
        assert_eq!(scaled.size.height, DevicePx(12.0));
    }
}
