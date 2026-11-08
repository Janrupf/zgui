//! The options a listbox holds, and which of them is being walked.

use std::collections::BTreeMap;

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, Memo, RenderEffect, RwSignal, StoredValue, UnsyncCallback};
use zgui_ui_primitives::{Binding, Collection, Controllable, ItemId};

use crate::listbox::catalogue::ListboxCatalogue;
use crate::listbox::keys::{ListboxAction, action_for};
use crate::listbox::labels::ListboxLabels;
use crate::listbox::option::{ListboxEntry, ListboxOption};
use crate::overlay::OverlayState;

/// Which option the arrow keys are on, as it was last asked for.
///
/// Kept as the request and resolved against the options when it is read, because a list opens
/// before its options mount: a highlight that resolved at once would find nothing to land on.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Active {
    /// The first option that can be chosen.
    First,
    /// The chosen option, or the first that can be chosen when no option has the value.
    Chosen,
    /// This option.
    Item(ItemId),
}

/// A set of options, the one being walked, and the one that is chosen.
///
/// The whole of what a select, a combobox and a command palette have in common. Three things it
/// keeps that nothing else can:
///
/// * **the options, in tree order** — announced by the options themselves, because a parent cannot
///   enumerate children that are behind a conditional or inside a list;
/// * **which one is active** — the one the arrow keys are on, which is *not* the one that has
///   focus: focus stays on the trigger or the text field, and the active option is named to a
///   reader by `active_descendant`;
/// * **which one is chosen** — controlled or not, exactly as everywhere else in this library.
///
/// ```
/// use zgui::prelude::*;
/// use zgui::reactive::{Mounted, install};
/// use zgui_ui::listbox::Listbox;
/// use zgui_ui::overlay::OverlayState;
/// use zgui_ui_primitives::Binding;
///
/// install().ok();
/// let scope = Mounted::new();
/// scope.with(|| {
///     let surface = OverlayState::uncontrolled(false, None);
///     let listbox = Listbox::new(surface, Binding::Unbound, None, None).provide();
///     assert!(listbox.chosen().is_empty(), "nothing is chosen yet");
///     assert!(listbox.active().is_none(), "and nothing is being walked");
///     assert!(Listbox::current().is_some());
/// });
/// scope.unmount();
/// ```
#[derive(Copy, Clone)]
pub struct Listbox {
    /// The options, in the order a reader meets them.
    collection: Collection,
    /// What is known about each of them.
    options: RwSignal<BTreeMap<ItemId, ListboxOption>, LocalStorage>,
    /// Which one the arrow keys are asked to be on.
    active: RwSignal<Active, LocalStorage>,
    /// The same, resolved against the options once per change, so each option compares an id.
    resolved: Memo<Option<ItemId>>,
    /// Which one is chosen.
    value: Controllable<String>,
    /// What has been typed to narrow the list, for the controls that have a field.
    filter: RwSignal<String, LocalStorage>,
    /// Whether the filter narrows the list: from the first keystroke until the query is reset.
    query_active: RwSignal<bool, LocalStorage>,
    /// What each value reads as, learned from the options that say so.
    ///
    /// A closed select has no options mounted: they are its list, and its list is gone. Without
    /// this the control would show its placeholder over a value it already has, from the moment it
    /// is built until the list has been opened once — which is the whole of the time anybody looks
    /// at it.
    labels: ListboxLabels,
    /// The surface the options are on.
    surface: OverlayState,
    /// Whether choosing something closes that surface.
    dismisses: bool,
    /// Whether the active option is to be scrolled into view once its element exists.
    ///
    /// Set by the keyboard, by opening the list on the chosen option and by the filter. A pointer
    /// that moves over an option leaves it clear, so the list does not move under the pointer.
    reveal: RwSignal<bool, LocalStorage>,
}

impl Listbox {
    /// Wires a listbox up to an overlay and to a value's three props.
    #[must_use]
    pub fn new(
        surface: OverlayState,
        value: Binding<String>,
        default_value: Option<String>,
        on_change: Option<UnsyncCallback<String>>,
    ) -> Self {
        let collection = Collection::new();
        let options = RwSignal::new_local(BTreeMap::new());
        let active = RwSignal::new_local(Active::First);
        let value = Controllable::new(value, default_value.unwrap_or_default(), on_change);
        let chosen = value.signal();
        let resolved = Memo::new(move |_| {
            resolve(
                active.get(),
                || entries_of(collection, options, true),
                || chosen.get(),
            )
        });
        let listbox = Self {
            collection,
            options,
            active,
            resolved,
            value,
            filter: RwSignal::new_local(String::new()),
            query_active: RwSignal::new_local(false),
            labels: ListboxLabels::new(),
            surface,
            dismisses: true,
            reveal: RwSignal::new_local(false),
        };
        listbox.reveal_active();
        listbox
    }

    /// Scrolls the active option into view each time [`Listbox::reveal`] is set.
    ///
    /// The effect reads nothing else while the flag is clear. When it is set, the effect waits for
    /// the option's element, because a list that opens in this turn mounts its options later.
    fn reveal_active(&self) {
        let listbox = *self;
        let revealing = RenderEffect::new(move |_| {
            if !listbox.reveal.get() {
                return;
            }
            let node = listbox.active_node();
            if node.get().is_none() {
                return;
            }
            node.scroll_to(ScrollTarget::IntoView, ScrollBehavior::Instant);
            listbox.reveal.set(false);
        });
        // Stored, so the effect lives as long as the scope that made the listbox.
        StoredValue::new_local(revealing);
    }

    /// Asks for the active option to be scrolled into view.
    fn reveal(&self) {
        if !self.reveal.get_untracked() {
            self.reveal.set(true);
        }
    }

    /// The same, for a list that is always on the surface it is written on.
    ///
    /// A command palette is one: its list is not a popup, so choosing something must not try to
    /// close anything — and a list that closed itself would answer the next arrow key by opening
    /// again instead of moving.
    #[must_use]
    pub const fn inline(mut self) -> Self {
        self.dismisses = false;
        self
    }

    /// Publishes this, and the collection its options register with, to every scope below.
    pub fn provide(self) -> Self {
        provide_local_context(self);
        provide_local_context(self.collection);
        self
    }

    /// The listbox the calling scope is inside, when it is inside one.
    #[must_use]
    pub fn current() -> Option<Self> {
        use_local_context::<Self>()
    }

    /// The surface the options are on.
    #[must_use]
    pub fn surface(&self) -> OverlayState {
        self.surface
    }

    /// Learns what one option's value reads as, without putting it on the list.
    ///
    /// What an option built inside a [`ListboxCatalogueOf`](crate::listbox::ListboxCatalogueOf)
    /// does instead of registering: it is not on the screen, so it is not something to walk to or
    /// to choose, and the only thing wanted from it is the one thing only it can say.
    pub fn describe(&self, option: ListboxOption) {
        self.labels.learn(option);
    }

    /// Learns that `value` reads as `text`, for an option that knows its text without an element.
    pub fn remember(&self, value: &str, text: &str) {
        self.labels.remember(value, text);
    }

    /// Registers one option, and takes it out again when the calling scope goes away.
    pub fn register(&self, option: ListboxOption) -> ItemId {
        let id = self.collection.register(option.node());
        self.options.update(|options| {
            options.insert(id, option);
        });
        let listbox = *self;
        on_cleanup_local(move || {
            listbox.options.try_update(|options| {
                options.remove(&id);
            });
        });
        id
    }

    /// Every option, in tree order.
    #[must_use]
    pub fn entries(&self) -> Vec<ListboxEntry> {
        entries_of(self.collection, self.options, true)
    }

    /// The same, without subscribing to the set.
    ///
    /// What the keyboard model reads: a key handler runs outside any reactive scope, and a read
    /// that tried to subscribe there would subscribe nothing to anything.
    #[must_use]
    pub fn entries_untracked(&self) -> Vec<ListboxEntry> {
        entries_of(self.collection, self.options, false)
    }

    /// Every option that can actually be chosen, in tree order, without subscribing.
    #[must_use]
    pub fn choosable(&self) -> Vec<ListboxEntry> {
        self.entries_untracked()
            .into_iter()
            .filter(|entry| !entry.option().is_disabled())
            .collect()
    }

    /// Which option the arrow keys are on.
    ///
    /// Until something has been walked or chosen, that is the **first** option that can be
    /// chosen — so an open list always has somewhere for `active_descendant` to point and for
    /// <kbd>Enter</kbd> to land, rather than a first arrow key that appears to do nothing. After
    /// [`highlight_chosen`](Self::highlight_chosen), it is the chosen option.
    #[must_use]
    pub fn active(&self) -> Option<ItemId> {
        self.resolved.get()
    }

    /// The same, without subscribing.
    #[must_use]
    fn active_untracked(&self) -> Option<ItemId> {
        self.resolved.get_untracked()
    }

    /// The element of the option the arrow keys are on, which is what a reader is pointed at.
    #[must_use]
    pub fn active_node(&self) -> NodeRef {
        let active = self.active();
        self.entries()
            .into_iter()
            .find(|entry| Some(entry.id()) == active)
            .map_or_else(NodeRef::new, |entry| entry.option().node())
    }

    /// Makes `id` the one the arrow keys are on, or the first option that can be chosen for
    /// `None`.
    pub fn set_active(&self, id: Option<ItemId>) {
        self.ask(id.map_or(Active::First, Active::Item));
    }

    /// Records where the arrow keys are asked to be, when that changes.
    fn ask(&self, target: Active) {
        if self.active.get_untracked() != target {
            self.active.set(target);
        }
    }

    /// Which value is chosen.
    #[must_use]
    pub fn chosen(&self) -> String {
        self.value.get()
    }

    /// Whether `value` is the chosen one.
    #[must_use]
    pub fn is_chosen(&self, value: &str) -> bool {
        self.value.get() == value
    }

    /// What the chosen option reads as, for a trigger to show.
    ///
    /// From the option itself while the list is mounted, and from what the value has been said to
    /// read as otherwise — because a closed select has no options at all, and a control that
    /// answered `None` there would show its placeholder over a value the user has already chosen.
    #[must_use]
    pub fn chosen_text(&self) -> Option<String> {
        let chosen = self.value.get();
        if chosen.is_empty() {
            return None;
        }
        self.entries()
            .into_iter()
            .find(|entry| entry.option().value() == chosen)
            .map(|entry| entry.option().text())
            .filter(|text| !text.is_empty())
            .or_else(|| self.labels.of(&chosen))
    }

    /// Chooses `value`, and tells whoever asked to be told.
    pub fn choose(&self, value: &str) {
        self.value.set(value.to_owned());
    }

    /// Chooses one option: reports its value, runs whatever it does, and closes the list.
    ///
    /// The one path everything takes — a press on an option, <kbd>Enter</kbd> on the control —
    /// so a palette cannot end up running a command on one of the two and not on the other.
    pub fn take(&self, id: ItemId) {
        let Some(entry) = self
            .entries_untracked()
            .into_iter()
            .find(|entry| entry.id() == id)
        else {
            return;
        };
        if entry.option().is_disabled() {
            return;
        }
        self.labels
            .remember(entry.option().value(), &entry.option().text());
        self.choose(entry.option().value());
        entry.option().select();
        if self.dismisses {
            self.surface.close();
        }
    }

    /// What has been typed to narrow the list.
    #[must_use]
    pub fn filter(&self) -> String {
        self.filter.get()
    }

    /// Narrows the list, and puts the highlight back on the first option that survives.
    ///
    /// Both, because they are one act: a highlight left on an option the filter has just taken
    /// away is a highlight on nothing, and Enter would choose something the user cannot see.
    pub fn set_filter(&self, text: impl Into<String>) {
        self.filter.set(text.into());
        if !self.query_active.get_untracked() {
            self.query_active.set(true);
        }
        self.ask(Active::First);
        self.reveal();
    }

    /// Whether something has been typed since the query was last reset.
    #[must_use]
    pub fn query_active(&self) -> bool {
        self.query_active.get()
    }

    /// Ends the query: clears the filter, so every option survives it again.
    ///
    /// What a combobox does once its list has gone, so the next opening shows every option.
    pub fn reset_query(&self) {
        if self.filter.try_with_untracked(String::is_empty) == Some(false) {
            self.filter.try_set(String::new());
        }
        if self.query_active.try_get_untracked() == Some(true) {
            self.query_active.try_set(false);
        }
    }

    /// Whether an option reading as `text` survives the filter.
    ///
    /// A substring rather than a prefix, and deliberately: a command palette is searched by any
    /// word of a command, not only by how it starts. Every option survives while no query is
    /// active.
    #[must_use]
    pub fn matches(&self, text: &str) -> bool {
        if !self.query_active.get() {
            return true;
        }
        let filter = self.filter.get();
        filter.is_empty() || text.to_lowercase().contains(&filter.to_lowercase())
    }

    /// Moves the highlight `steps` places among the options that can be chosen, wrapping.
    ///
    /// From nothing, a forward step lands on the first and a backward one on the last, which is
    /// what makes the down arrow open a select onto its first option.
    pub fn step(&self, steps: isize) {
        let entries = self.choosable();
        if entries.is_empty() {
            return;
        }
        let length = entries.len() as isize;
        let from = self
            .active_untracked()
            .and_then(|id| entries.iter().position(|entry| entry.id() == id));
        let at = match from {
            Some(at) => at as isize + steps,
            None if steps >= 0 => 0,
            None => length - 1,
        };
        let index = at.rem_euclid(length) as usize;
        self.set_active(Some(entries[index].id()));
        self.reveal();
    }

    /// Moves the highlight to the last option that can be chosen, or the first.
    pub fn to_end(&self, last: bool) {
        let entries = self.choosable();
        let target = if last {
            entries.last()
        } else {
            entries.first()
        };
        if let Some(entry) = target {
            self.set_active(Some(entry.id()));
            self.reveal();
        }
    }

    /// Puts the highlight on the chosen option, or on the first choosable one.
    ///
    /// What opening the list does: a select opened on *Amount* highlights *Amount*, so one press
    /// of the down arrow moves to the next one rather than back to the top. The options of a list
    /// that opens in the same turn mount later, so the highlight finds its option when it is read.
    pub fn highlight_chosen(&self) {
        self.ask(Active::Chosen);
        self.reveal();
    }

    /// Carries out what a key press means, and reports whether anything came of it.
    ///
    /// The one place the whole keyboard model lives, so a select, a combobox and a palette cannot
    /// drift apart on what the down arrow does.
    pub fn handle(&self, key: &zgui::vocab::Key) -> bool {
        let open = self.surface.is_open_untracked();
        let Some(action) = action_for(key, open) else {
            return false;
        };
        match action {
            ListboxAction::Step(steps) => {
                if open {
                    self.step(steps);
                } else {
                    self.surface.open();
                    self.highlight_chosen();
                }
            }
            ListboxAction::End(last) => self.to_end(last),
            ListboxAction::Choose => {
                let Some(active) = self.active_untracked() else {
                    return false;
                };
                self.take(active);
            }
            ListboxAction::Close => {
                if !self.dismisses {
                    return false;
                }
                self.surface.close();
            }
        }
        true
    }
}

/// The option `target` names among `entries`, with the chosen value from `chosen`.
///
/// Reads the entries and the chosen value only when `target` needs them.
fn resolve(
    target: Active,
    entries: impl FnOnce() -> Vec<ListboxEntry>,
    chosen: impl FnOnce() -> String,
) -> Option<ItemId> {
    if let Active::Item(id) = target {
        return Some(id);
    }
    let entries = entries();
    let mut choosable = entries.iter().filter(|entry| !entry.option().is_disabled());
    let found = match target {
        Active::Chosen => {
            let chosen = chosen();
            choosable
                .clone()
                .find(|entry| entry.option().value() == chosen)
        }
        Active::First | Active::Item(_) => None,
    };
    found.or_else(|| choosable.next()).map(ListboxEntry::id)
}

/// Every option of `collection` that `options` knows about, in tree order.
///
/// `tracked` subscribes the calling scope to both.
fn entries_of(
    collection: Collection,
    options: RwSignal<BTreeMap<ItemId, ListboxOption>, LocalStorage>,
    tracked: bool,
) -> Vec<ListboxEntry> {
    let (items, known) = if tracked {
        (collection.items(), options.get())
    } else {
        (collection.items_untracked(), options.get_untracked())
    };
    items
        .into_iter()
        .filter_map(|item| {
            known
                .get(&item.id())
                .map(|option| ListboxEntry::new(item.id(), option.clone()))
        })
        .collect()
}

/// Registers `node` as an option of the enclosing [`Listbox`], when there is one.
///
/// `None` outside one, which is an ordinary answer: the same option component still renders and
/// still says what it is, and there is simply nothing for it to be an option of. `None` too inside
/// a [`ListboxCatalogueOf`](crate::listbox::ListboxCatalogueOf), where the option is built to be
/// read rather than to be chosen — so an option written once cannot end up on the list twice.
pub fn use_listbox_option(option: ListboxOption) -> Option<(Listbox, ItemId)> {
    let listbox = Listbox::current()?;
    if ListboxCatalogue::is_current() {
        listbox.describe(option);
        return None;
    }
    let id = listbox.register(option);
    Some((listbox, id))
}
