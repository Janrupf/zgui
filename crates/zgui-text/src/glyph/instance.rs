//! Which point in a variable face's design space a run is drawn at.

/// One interned set of normalised variation coordinates.
///
/// A variable face draws a different outline at each point of its design space, so a glyph is
/// identified by its face, its index and this. The shaper interns the coordinates it shaped a run
/// at and hands out the number; the rasteriser that produced the number turns it back into the
/// coordinates. Zero is the face's default instance, which is what every static face is drawn at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VariationInstance(pub u32);

impl VariationInstance {
    /// The face's default instance.
    pub const DEFAULT: Self = Self(0);

    /// Whether this is the face's default instance.
    pub fn is_default(self) -> bool {
        self == Self::DEFAULT
    }
}
