//! Lines: turning a run of text, images and nested spans into boxes with positions.
//!
//! # What is a leaf here and what is not
//!
//! To the algorithms around it an inline formatting context is one leaf box with a size. Inside it
//! is a tree — text runs, images, `inline-block`s, spans nested inside spans, each with its own
//! font, margins and alignment — and none of those is laid out on its own. The box that
//! *establishes* the context lays all of them out together, in one pass over one shaped paragraph,
//! because a line break is a decision about the whole sequence and cannot be taken piecewise.
//!
//! A box whose parent already establishes a context is therefore never asked for its own size. It
//! is reached through its context and nowhere else.
//!
//! | Module | What it decides |
//! |---|---|
//! | [`strut`] | how tall each run makes the line it is on |
//! | [`lines`] | the CSS line boxes, and where they stack |
//! | [`ellipsis`] | where a line that does not fit is cut off, and what marks the cut |
//! | [`vertical_align`] | how far each box moves off the baseline |
//! | [`floats`] | how wide each line is allowed to be |
//! | [`baseline`] | which line a parent aligns the whole context by |
//! | [`resolved`] | what the context came out at, for everything that reads layout afterwards |
//! | [`slices`] | where each nested inline box lands on each line, for painting |
//! | [`atomic`] | the nested layout an `inline-block` costs, and the memo without which it is ruinous |
//!
//! Flattening the tree into the string a shaper takes, sizing what is on the line that is not a
//! glyph, and emitting one fragment per line are internal to the context: none of the three is a
//! question anything outside it asks.

pub mod atomic;
pub mod baseline;
pub(crate) mod boxes;
pub(crate) mod content;
pub mod ellipsis;
pub mod floats;
pub(crate) mod insets;
pub mod kept;
pub mod leaf_shell;
pub mod lines;
pub(crate) mod measure;
pub mod resolved;
pub mod slices;
pub mod strut;
pub mod vertical_align;

use std::sync::Arc;

use cephal::Size;
use cephal::compute::block::BlockContext;
use cephal::tree::{NodeId, RequestedAxis, RunMode};
use zgui_dom::side::BoxKey;

use crate::inline::content::Generated;
use crate::inline::content::memo::Flattened;
use crate::key::from_node_id;
use crate::measure::{MeasureContent, MeasureRequest, Measured};
use crate::node::kind::FormattingContext;
use crate::tree::LayoutTree;

/// Measures one leaf box's content.
///
/// Two very different things arrive here. An inline formatting context is laid out in full, because
/// its size *is* the lines it broke into. Replaced content — an image, a video, an embedded surface
/// — is asked of whoever is driving the pass, because this engine does not own it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn measure_leaf<C: MeasureContent>(
    tree: &mut LayoutTree<'_, C>,
    node: NodeId,
    known: Size<Option<f32>>,
    available: Size<cephal::AvailableSpace>,
    run_mode: RunMode,
    axis: RequestedAxis,
    basis_height: Option<f32>,
    block: Option<&mut BlockContext<'_>>,
) -> Measured {
    let key = from_node_id(node);
    // A speculative pass asks for a whole layout to read a baseline or a float's extent from it,
    // and the engine keeps none of it. Its answer is a probe here, so it writes nothing.
    let final_pass =
        run_mode == RunMode::PerformLayout && cephal::tree::CacheAccess::speculation(tree) == 0;
    if tree.structure().node(key).fc == FormattingContext::Custom {
        // A custom element's measurement is the trait's, not the measurer's: the source was
        // installed on the pass, and the shell arithmetic around this call applies to its answer
        // exactly as it applies to a replaced box's.
        return crate::custom::measure(tree, key, known, available, final_pass);
    }
    if tree.structure().node(key).fc == FormattingContext::Inline {
        return measure::compute(
            tree,
            key,
            measure::Ask {
                known,
                available,
                run_mode,
                axis,
                final_pass,
                basis_height,
            },
            block,
        );
    }
    let scale = tree.device().scale;
    let style = tree.structure().node(key).style.clone();
    let natural = tree
        .structure()
        .replaced(key)
        .and_then(|content| content.natural);
    tree.content().measure(MeasureRequest {
        box_: key,
        style: &style,
        known,
        available,
        natural,
        scale,
        final_pass,
    })
}

/// The flattened form of one inline formatting context, built if the box is not already holding one
/// that was built from the same content.
///
/// Flattening walks every character in the context, and what comes out depends on the boxes, their
/// styles and the device scale — never on the width the context is being asked about. The
/// algorithms around a paragraph ask it how big it is at a dozen widths, and lay the document out
/// again whenever anything anywhere moves, so the flattened form is kept beside the box and reused
/// until the content it was built from is no longer what the box holds.
pub(crate) fn content_of<C: MeasureContent>(
    tree: &mut LayoutTree<'_, C>,
    key: BoxKey,
) -> Arc<Generated> {
    let scale = tree.device().scale;
    let pieces = content::collect::pieces(tree.structure(), key);
    if let Some(held) = tree
        .flattened_of(key)
        .and_then(|held| held.reuse(scale, &pieces))
    {
        return held;
    }
    let built = {
        let (store, measurer, styles) = tree.content_parts();
        let mut claim = |paint: &zgui_text_style::TextPaint| measurer.paint_slot(paint);
        Arc::new(content::generate::build(
            store, key, &pieces, styles, &mut claim, scale,
        ))
    };
    tree.hold_flattened(key, Flattened::new(scale, pieces, Arc::clone(&built)));
    built
}
