//! How many options of one group are mounted.

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, RwSignal};

/// Counts the mounted options of one group, so the group can hide when a filter empties it.
///
/// An option counts itself in when it mounts and out when it goes. A group of a filtered list
/// reads [`is_empty`](Self::is_empty) and hides its heading with its options.
///
/// ```
/// use zgui::reactive::{Mounted, install};
/// use zgui_ui::listbox::ListboxGroup;
///
/// install().ok();
/// let scope = Mounted::new();
/// scope.with(|| {
///     let group = ListboxGroup::new().provide();
///     assert!(group.is_empty());
///     let option = Mounted::new();
///     option.with(|| ListboxGroup::current().expect("provided").count_in());
///     assert!(!group.is_empty());
///     option.unmount();
///     assert!(group.is_empty());
/// });
/// scope.unmount();
/// ```
#[derive(Copy, Clone)]
pub struct ListboxGroup {
    /// How many options are mounted.
    mounted: RwSignal<u32, LocalStorage>,
}

impl Default for ListboxGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl ListboxGroup {
    /// A group with no options mounted.
    #[must_use]
    pub fn new() -> Self {
        Self {
            mounted: RwSignal::new_local(0),
        }
    }

    /// Publishes this to every scope below, and hands it back.
    pub fn provide(self) -> Self {
        provide_local_context(self);
        self
    }

    /// The group the calling scope is inside, when it is inside one.
    #[must_use]
    pub fn current() -> Option<Self> {
        use_local_context::<Self>()
    }

    /// Counts one option in, and out again when the calling scope goes away.
    pub fn count_in(&self) {
        let mounted = self.mounted;
        mounted.update(|count| *count += 1);
        on_cleanup_local(move || {
            mounted.try_update(|count| *count = count.saturating_sub(1));
        });
    }

    /// Whether no option of the group is mounted, subscribing to it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mounted.get() == 0
    }
}
