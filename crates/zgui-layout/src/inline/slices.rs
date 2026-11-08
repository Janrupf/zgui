//! Where each nested inline box lands on each line it crosses.
//!
//! A nested inline box has no fragment of its own size: its text is glyphs in the lines of the
//! context, and its edges are opaque boxes the shaper packed between the words. Painting its
//! background, border and decorations needs its rectangle on each line, so the rectangle is
//! recovered here from where its two edges landed.
//!
//! The rectangles are kept on the resolution and read by the line that paints them. They never
//! change a line box: the block-axis extent is the box's own content area plus its vertical padding
//! and border, which CSS lets overflow the line.

use core::ops::Range;

use zgui_dom::side::BoxKey;
use zgui_geom::Edges;
use zgui_text::BrokenParagraph;

use crate::inline::boxes::Boxes;
use crate::inline::content::{Generated, Role};
use crate::inline::insets::frame_of;
use crate::inline::lines::LineBox;
use crate::inline::vertical_align::scale_strut;
use crate::measure::MeasureContent;
use crate::tree::LayoutTree;

/// The part of one nested inline box that lies on one line.
///
/// All lengths are in the units layout works in, and positions are relative to the context's
/// content box.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineSlice {
    /// The nested inline box.
    pub box_: BoxKey,
    /// The line this part is on.
    pub line: u32,
    /// The left edge of the border box.
    pub x: f32,
    /// The top edge of the border box.
    pub y: f32,
    /// The border box's width.
    pub width: f32,
    /// The border box's height.
    pub height: f32,
    /// The border widths of this part. A side the box is broken on is zero.
    pub border: Edges<f32>,
    /// The padding of this part. A side the box is broken on is zero.
    pub padding: Edges<f32>,
    /// Whether this part holds the box's start edge.
    pub first: bool,
    /// Whether this part holds the box's end edge.
    pub last: bool,
    /// The range of the generated string the box holds, over all its lines.
    pub text: Range<u32>,
    /// How far the box's baseline is raised above the line's, in whole device pixels.
    pub shift: f32,
    /// The box's own baseline, after the shift.
    pub baseline: f32,
    /// The box's font ascent.
    pub ascent: f32,
    /// The box's font descent.
    pub descent: f32,
    /// The box's font x-height.
    pub x_height: f32,
    /// The box's font size.
    pub font_size: f32,
}

impl InlineSlice {
    /// The x range of the content inside this part.
    pub fn content_x(&self) -> (f32, f32) {
        (
            self.x + self.border.left + self.padding.left,
            self.x + self.width - self.border.right - self.padding.right,
        )
    }
}

/// One nested inline box, by the indices of its two edge items.
struct Span {
    /// The box.
    key: BoxKey,
    /// Its start edge's item index.
    start: usize,
    /// Its end edge's item index, once it is known.
    end: Option<usize>,
}

/// The parts of every nested inline box, ordered by line and then by tree order.
///
/// `boxes` is the geometry the paragraph was broken with and `basis` the width percentages resolve
/// against, both as the boxes were resolved for this pass.
pub(crate) fn compute<C: MeasureContent>(
    tree: &mut LayoutTree<'_, C>,
    generated: &Generated,
    boxes: &Boxes,
    lines: &[LineBox],
    broken: &BrokenParagraph,
    basis: Option<f32>,
) -> Vec<InlineSlice> {
    let mut spans: Vec<Span> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    for (index, item) in generated.items.iter().enumerate() {
        match item.role {
            Role::StartEdge(key) => {
                open.push(spans.len());
                spans.push(Span {
                    key,
                    start: index,
                    end: None,
                });
            }
            Role::EndEdge(_) => {
                if let Some(span) = open.pop() {
                    spans[span].end = Some(index);
                }
            }
            Role::Atomic(_) => {}
        }
    }

    // Item identifiers run from zero in item order, so a placement finds its item by index.
    let mut placed: Vec<Option<(f32, usize)>> = vec![None; generated.items.len()];
    for placement in &broken.boxes {
        if let Some(slot) = usize::try_from(placement.id)
            .ok()
            .and_then(|id| placed.get_mut(id))
        {
            *slot = Some((placement.origin.x.0, placement.line));
        }
    }

    let scale = tree.device().scale;
    let mut out = Vec::new();
    for span in &spans {
        let Some(end) = span.end else {
            continue;
        };
        let (Some((start_x, start_line)), Some((end_x, end_line))) =
            (placed[span.start], placed[end])
        else {
            continue;
        };
        let start_offset = generated.items[span.start].offset;
        let end_offset = generated.items[end].offset;
        // An edge that landed at the end of the line before its text moves to the line its text
        // starts on, and an edge that landed at the start of the line after its text moves back.
        let (mut first_line, mut last_line) = (start_line, end_line);
        let mut start_x = Some(start_x);
        let mut end_x = Some(end_x);
        if first_line < last_line
            && lines
                .get(first_line)
                .is_some_and(|line| start_offset >= line.text.end)
        {
            first_line += 1;
            start_x = None;
        }
        if last_line > first_line
            && lines
                .get(last_line)
                .is_some_and(|line| end_offset <= line.text.start)
        {
            last_line -= 1;
            end_x = None;
        }

        let (margin, padding, border) = frame_of(tree, span.key, basis);
        let style = tree.structure().node(span.key).style.clone();
        let run = tree.text_styles_mut().get(&style);
        let strut = scale_strut(tree.content().strut(&run.text), scale);
        let shift = boxes
            .shifts
            .get(span.start)
            .map_or(0.0, |shift| shift.round());

        for line_index in first_line..=last_line {
            let Some(line) = lines.get(line_index) else {
                break;
            };
            let first = line_index == first_line;
            let last = line_index == last_line;
            let start_inset = border.left + padding.left;
            let end_inset = border.right + padding.right;
            let mut left = match (first, start_x) {
                (true, Some(x)) => x + margin.left,
                (true, None) => line.offset - start_inset,
                (false, _) => line.offset,
            };
            let mut right = match (last, end_x) {
                (true, Some(x)) => x + end_inset,
                (true, None) => line.offset + line.width + end_inset,
                (false, _) => line.offset + line.width,
            };
            // A right-to-left box has its start edge on the right; its parts are measured between
            // the two edges either way.
            if right < left {
                core::mem::swap(&mut left, &mut right);
            }
            if let Some(cut) = line.ellipsis {
                if cut.at_start {
                    left = left.max(cut.cutoff);
                } else {
                    right = right.min(cut.cutoff);
                }
                if right <= left {
                    continue;
                }
            }
            let sliced_border = Edges {
                left: if first { border.left } else { 0.0 },
                right: if last { border.right } else { 0.0 },
                top: border.top,
                bottom: border.bottom,
            };
            let sliced_padding = Edges {
                left: if first { padding.left } else { 0.0 },
                right: if last { padding.right } else { 0.0 },
                top: padding.top,
                bottom: padding.bottom,
            };
            let baseline = line.baseline() - shift;
            let top = baseline - strut.font_ascent.0 - padding.top - border.top;
            let height = strut.font_ascent.0
                + strut.font_descent.0
                + padding.top
                + padding.bottom
                + border.top
                + border.bottom;
            out.push(InlineSlice {
                box_: span.key,
                line: u32::try_from(line_index).unwrap_or(u32::MAX),
                x: left,
                y: top,
                width: right - left,
                height,
                border: sliced_border,
                padding: sliced_padding,
                first,
                last,
                text: u32::try_from(start_offset).unwrap_or(u32::MAX)
                    ..u32::try_from(end_offset).unwrap_or(u32::MAX),
                shift,
                baseline,
                ascent: strut.font_ascent.0,
                descent: strut.font_descent.0,
                x_height: strut.x_height.0,
                font_size: strut.font_size.0,
            });
        }
    }
    // Stable, so the parts on one line keep tree order: an outer box before the boxes inside it.
    out.sort_by_key(|slice| slice.line);
    out
}

/// The parts that lie on one line, in tree order.
pub fn on_line(slices: &[InlineSlice], line: usize) -> &[InlineSlice] {
    let line = u32::try_from(line).unwrap_or(u32::MAX);
    let start = slices.partition_point(|slice| slice.line < line);
    let end = slices.partition_point(|slice| slice.line <= line);
    &slices[start..end]
}

/// One part's border box on the device, given the context's content box there.
///
/// Every edge is rounded to a whole device pixel. The fragment's ink and the painted rectangle both
/// come from here, so a part that moves leaves no trail.
pub fn device_rect(
    content_box: zgui_geom::Rect<zgui_geom::DevicePx, zgui_geom::Device>,
    slice: &InlineSlice,
) -> zgui_geom::Rect<zgui_geom::DevicePx, zgui_geom::Device> {
    use zgui_geom::{DevicePx, Point};
    let left = (content_box.origin.x.0 + slice.x).round();
    let top = (content_box.origin.y.0 + slice.y).round();
    let right = (content_box.origin.x.0 + slice.x + slice.width).round();
    let bottom = (content_box.origin.y.0 + slice.y + slice.height).round();
    zgui_geom::Rect::from_corners(
        Point::new(DevicePx(left), DevicePx(top)),
        Point::new(DevicePx(right.max(left)), DevicePx(bottom.max(top))),
    )
}
