//! Hashing a side-table entry by what it *is*.
//!
//! The three interned tables key their entries on content, and their entries hold floating-point
//! numbers — colour channels, gradient angles, corner radii. `Hash` and `Eq` are unavailable for
//! those, so this module defines the pair the tables actually need: an exact bit-pattern hash, and
//! ordinary structural equality to settle collisions.
//!
//! Hashing bit patterns rather than values means `0.0` and `-0.0` are different content, and two
//! `NaN`s with the same payload are the same content. Both are correct here: the question a table
//! asks is "have I already stored exactly these bytes", not "are these numerically equal".

/// A value a side table can intern.
///
/// Equality decides whether two entries are the same, and the hash only narrows the search — so an
/// implementation may hash conservatively, but two equal values must hash the same.
pub trait Content: Clone + PartialEq {
    /// A hash of everything equality looks at.
    fn content_hash(&self) -> u64;

    /// Whether replacing a table entry with `other` changes the value a reader observes.
    ///
    /// Usually this is the same question as equality. A value whose stable interned identity is
    /// deliberately narrower than what it stores can override it: a clip node, for example, keeps
    /// one id while scrolling rewrites where its rectangle is drawn.
    fn same_stored_value(&self, other: &Self) -> bool {
        self == other
    }
}

/// An incremental hash over the raw bytes of a value's fields.
///
/// It is FNV-1a: two multiplications and an exclusive-or per byte, no state beyond a `u64`, and
/// good enough for a table whose collisions are settled by comparison anyway. It is here rather
/// than `DefaultHasher` because a stored content hash is compared *across frames*, and a hash whose
/// seed changes per process could not be.
#[derive(Clone, Copy, Debug)]
pub struct ContentHash(u64);

impl Default for ContentHash {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentHash {
    /// FNV-1a's 64-bit offset basis.
    const BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    /// FNV-1a's 64-bit prime.
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    /// A hash of nothing yet.
    pub const fn new() -> Self {
        Self(Self::BASIS)
    }

    /// Folds in every byte of `bytes`, one round apiece.
    ///
    /// For a run of bytes whose length is itself content — a name, a string — where a round per
    /// byte is what keeps two different lengths apart. A fixed-width field belongs in
    /// [`ContentHash::u64`] instead, which folds the whole word in one round; the two are separate
    /// foldings and are not expected to agree on the same bytes.
    pub const fn bytes(mut self, bytes: &[u8]) -> Self {
        let mut index = 0;
        while index < bytes.len() {
            self.0 ^= bytes[index] as u64;
            self.0 = self.0.wrapping_mul(Self::PRIME);
            index += 1;
        }
        self
    }

    /// Folds in a `u64`, as one round rather than as eight.
    ///
    /// A fixed-width field is folded whole. Doing it a byte at a time cost eight rounds, and each
    /// round is a multiply that waits on the one before it — so a matrix, which is sixteen of these
    /// fields, was a chain of a hundred and twenty-eight dependent multiplies to hash sixty-four
    /// bytes. Half of those rounds were folding in the zero padding of a widened `f32`.
    ///
    /// The shift after the multiply is load-bearing rather than decorative. A multiply carries
    /// upwards and never down, and the top bit is a fixed point of it — `2^63` times any odd number
    /// is `2^63` again — so without it, flipping the top bit of *any* field gave one hash whichever
    /// field it was flipped in. The shift folds the high half back over the low one, which the next
    /// round's multiply then carries up again.
    pub const fn u64(mut self, value: u64) -> Self {
        self.0 ^= value;
        self.0 = self.0.wrapping_mul(Self::PRIME);
        self.0 ^= self.0 >> 31;
        self
    }

    /// Folds in a `u32`.
    pub const fn u32(self, value: u32) -> Self {
        self.u64(value as u64)
    }

    /// Folds in an `i32`.
    pub const fn i32(self, value: i32) -> Self {
        self.u64(value as u32 as u64)
    }

    /// Folds in an `f32` by its bit pattern.
    pub const fn f32(self, value: f32) -> Self {
        self.u32(value.to_bits())
    }

    /// Folds in every element of a slice of `f32`, by bit pattern.
    pub fn f32s(mut self, values: &[f32]) -> Self {
        for value in values {
            self = self.f32(*value);
        }
        self
    }

    /// The hash so far, spread so that every input bit reaches every output bit.
    ///
    /// A round leaves the lowest bit of the state as the exclusive-or of the lowest bit of
    /// everything folded in, because multiplication carries upwards and never down. A consumer
    /// bucketing on the low bits would see far fewer than sixty-four bits of hash. This is one
    /// multiply for the whole value, rather than one per field, so it costs nothing measurable.
    pub const fn finish(self) -> u64 {
        let folded = self.0 ^ (self.0 >> 32);
        folded.wrapping_mul(0xff51_afd7_ed55_8ccd)
    }
}

/// A matrix is content: sixteen numbers, hashed as their bit patterns.
///
/// It is here rather than beside the coordinate systems that hold matrices because nothing about a
/// coordinate system is interned — what asks for this is a record that kept a fingerprint of the
/// matrix it drew through, so that a matrix which moved is re-encoded rather than replayed.
impl Content for zgui_geom::Matrix4 {
    fn content_hash(&self) -> u64 {
        let mut hash = ContentHash::new();
        for column in self.columns {
            hash = hash.f32s(&column);
        }
        hash.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::ContentHash;

    #[test]
    fn the_same_bytes_hash_the_same_every_time() {
        let once = ContentHash::new().f32(1.5).u32(7).finish();
        let again = ContentHash::new().f32(1.5).u32(7).finish();
        assert_eq!(once, again);
    }

    /// Folding a word whole is what makes the hash cheap, so the property that pays for it —
    /// every bit of every field reaching the result — is asserted rather than assumed. A field
    /// that flips one bit must change the hash, including the bit a multiply cannot carry into.
    #[test]
    fn one_flipped_bit_anywhere_changes_the_hash() {
        let base = ContentHash::new().u64(0).u64(0).finish();
        for bit in 0..64 {
            let first = ContentHash::new().u64(1 << bit).u64(0).finish();
            let second = ContentHash::new().u64(0).u64(1 << bit).finish();
            assert_ne!(first, base, "bit {bit} of the first field");
            assert_ne!(second, base, "bit {bit} of the second field");
            assert_ne!(
                first, second,
                "the two fields are not interchangeable at bit {bit}"
            );
        }
    }

    /// The sixteen numbers of a matrix must not collide across the values an animation walks
    /// through. A shift stepping a pixel at a time is exactly that walk.
    #[test]
    fn a_matrix_hashes_apart_across_a_shift_that_animates() {
        use super::Content;
        let mut seen = std::collections::HashSet::new();
        for step in 0..4096 {
            let by = step as f32 * 0.25;
            assert!(
                seen.insert(zgui_geom::Matrix4::translation(by, -by, 0.0).content_hash()),
                "two shifts an animation passes through hashed alike at step {step}",
            );
        }
    }

    #[test]
    fn field_order_is_part_of_the_content() {
        let forwards = ContentHash::new().u32(1).u32(2).finish();
        let backwards = ContentHash::new().u32(2).u32(1).finish();
        assert_ne!(forwards, backwards);
    }

    #[test]
    fn signed_zeroes_are_different_content() {
        assert_ne!(
            ContentHash::new().f32(0.0).finish(),
            ContentHash::new().f32(-0.0).finish()
        );
    }
}
