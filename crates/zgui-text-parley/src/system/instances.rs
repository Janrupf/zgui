//! The variation instances runs were shaped at, interned so a glyph key can name one.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use zgui_text::VariationInstance;

/// Every set of normalised coordinates a run was shaped at, numbered from one.
///
/// Zero is the default instance and is never stored: an empty set, and a set of zeros, are both
/// the face's default location. Entries are never removed, because a glyph key that names one may
/// be held in a cache for as long as the application runs, and the number of distinct instances a
/// document uses is small.
#[derive(Debug, Default)]
pub(crate) struct Instances {
    /// The coordinates of instance `n`, at index `n - 1`.
    coords: Vec<Arc<[i16]>>,
    /// The way back from coordinates to their number.
    by_coords: FxHashMap<Arc<[i16]>, VariationInstance>,
}

impl Instances {
    /// The number already issued for `coords`, if one was.
    pub(crate) fn find(&self, coords: &[i16]) -> Option<VariationInstance> {
        if is_default(coords) {
            return Some(VariationInstance::DEFAULT);
        }
        self.by_coords.get(coords).copied()
    }

    /// The number for `coords`, issuing one if none was.
    pub(crate) fn intern(&mut self, coords: &[i16]) -> VariationInstance {
        if let Some(found) = self.find(coords) {
            return found;
        }
        let held: Arc<[i16]> = Arc::from(coords);
        self.coords.push(Arc::clone(&held));
        let instance = VariationInstance(self.coords.len() as u32);
        self.by_coords.insert(held, instance);
        instance
    }

    /// The coordinates `instance` stands for, or `None` for the default instance and for a number
    /// this table did not issue.
    pub(crate) fn coords(&self, instance: VariationInstance) -> Option<Arc<[i16]>> {
        let index = instance.0.checked_sub(1)?;
        self.coords.get(index as usize).cloned()
    }
}

/// Whether `coords` is the default location.
fn is_default(coords: &[i16]) -> bool {
    coords.iter().all(|coord| *coord == 0)
}

#[cfg(test)]
mod tests {
    use super::Instances;
    use zgui_text::VariationInstance;

    #[test]
    fn the_default_location_is_instance_zero_however_it_is_spelled() {
        let mut table = Instances::default();
        assert_eq!(table.intern(&[]), VariationInstance::DEFAULT);
        assert_eq!(table.intern(&[0, 0]), VariationInstance::DEFAULT);
        assert!(table.coords(VariationInstance::DEFAULT).is_none());
    }

    #[test]
    fn one_location_is_one_number() {
        let mut table = Instances::default();
        let bold = table.intern(&[8192]);
        let light = table.intern(&[-4096]);
        assert_ne!(bold, light);
        assert_eq!(table.intern(&[8192]), bold);
        assert_eq!(table.coords(bold).as_deref(), Some(&[8192][..]));
    }
}
