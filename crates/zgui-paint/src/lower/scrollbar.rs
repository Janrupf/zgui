//! What a scroll container's style says about its scrollbars.
//!
//! Two inputs. The inherited `color-scheme` picks the light or the dark default paint, so a dark
//! theme on a light desktop gets a thumb that shows on its own background. The custom properties
//! [`THUMB`] and [`TRACK`] replace the default colours, the way `--zgui-fill` replaces a drawing's
//! fill.
//!
//! Only a scroll container reads either. Every other box lowers to the default, which keeps the
//! lowering of an ordinary box independent of both.

use std::sync::Arc;

use zgui_color::Color;
use zgui_css::parity::Support;
use zgui_css::values::color::ColorSchemeFlags;
use zgui_css::values::custom;
use zgui_css::{ComputedStyle, register_properties};

register_properties! {
    color_scheme => Support::Implemented("zgui-paint::lower::scrollbar"),
}

/// The custom property that sets the colour of a scroll container's thumb.
pub const THUMB: &str = "zgui-scrollbar-thumb";

/// The custom property that sets the colour of a scroll container's track.
pub const TRACK: &str = "zgui-scrollbar-track";

/// Which default paint a scroll container's bars start from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScrollbarScheme {
    /// The scheme the window is presented in.
    #[default]
    Window,
    /// The light paint, whatever the window's scheme.
    Light,
    /// The dark paint, whatever the window's scheme.
    Dark,
}

/// The colours a style sheet gave a scroll container's bars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarColors {
    /// The thumb's colour, when [`THUMB`] names one.
    pub thumb: Option<Color>,
    /// The track's colour, when [`TRACK`] names one.
    pub track: Option<Color>,
}

/// What one style says about the bars of the box it is on.
///
/// The default for every box that is not a scroll container. The colours are behind a pointer
/// because they are almost always absent and a lowering is cloned per fragment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScrollbarStyle {
    /// Which default paint the bars start from.
    pub scheme: ScrollbarScheme,
    /// The colours that replace the default ones, when a style sheet set any.
    pub colors: Option<Arc<ScrollbarColors>>,
}

/// The scrollbar style of `style`.
pub fn of(style: &ComputedStyle) -> ScrollbarStyle {
    if !zgui_layout::scroll_region::is_scroll_container(style) {
        return ScrollbarStyle::default();
    }
    let thumb = custom::color(style, THUMB);
    let track = custom::color(style, TRACK);
    ScrollbarStyle {
        scheme: scheme_of(style.get_inherited_ui().color_scheme.bits),
        colors: (thumb.is_some() || track.is_some())
            .then(|| Arc::new(ScrollbarColors { thumb, track })),
    }
}

/// The default paint a `color-scheme` value picks.
///
/// A value that names one scheme picks it. `normal`, and a value that names both, leave the choice
/// to the window: the content supports the scheme the window is presented in.
pub fn scheme_of(bits: ColorSchemeFlags) -> ScrollbarScheme {
    match (
        bits.contains(ColorSchemeFlags::LIGHT),
        bits.contains(ColorSchemeFlags::DARK),
    ) {
        (true, false) => ScrollbarScheme::Light,
        (false, true) => ScrollbarScheme::Dark,
        _ => ScrollbarScheme::Window,
    }
}

#[cfg(test)]
mod tests {
    use zgui_css::values::color::ColorSchemeFlags;

    use super::{ScrollbarScheme, scheme_of};

    #[test]
    fn one_named_scheme_picks_its_paint_and_anything_else_follows_the_window() {
        assert_eq!(
            scheme_of(ColorSchemeFlags::empty()),
            ScrollbarScheme::Window
        );
        assert_eq!(scheme_of(ColorSchemeFlags::LIGHT), ScrollbarScheme::Light);
        assert_eq!(scheme_of(ColorSchemeFlags::DARK), ScrollbarScheme::Dark);
        assert_eq!(
            scheme_of(ColorSchemeFlags::DARK | ColorSchemeFlags::ONLY),
            ScrollbarScheme::Dark
        );
        assert_eq!(
            scheme_of(ColorSchemeFlags::LIGHT | ColorSchemeFlags::DARK),
            ScrollbarScheme::Window
        );
    }
}
