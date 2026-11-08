//! Whether the keyboard keeps the option it lands on inside a list that scrolls.
//!
//! Each list here holds thirty options and is capped well short of that, so most of its options
//! are past the fold. Each fixture walks the list with the keyboard and then asks the layout where
//! the active option is: inside the list's box, or somewhere the list clips away.

mod desktop;
mod device;
mod painted;

use zgui::geom::{Device, DevicePx, Rect};
use zgui::view;
use zgui::view::AnyView;
use zgui::vocab::NamedKey;
use zgui_ui::prelude::*;
use zgui_ui_tokens::prelude::*;

use crate::painted::stage::{SETTLED, Stage};

/// The page every fixture is laid out on, with the control near the top so its list opens down.
const SHEET: &str = ":root { background-color: #ffffff; color: #101010; font-family: sans-serif }
                     .page { padding: 24px; gap: 24px; align-items: flex-start }";

/// How many options each list holds.
const COUNT: usize = 30;

/// Opens `view`, or reports the run skipped on a machine with no graphics device.
macro_rules! staged {
    ($view:expr) => {
        match Stage::open(SHEET, $view) {
            Some(stage) => stage,
            None => {
                eprintln!("skipped: no usable graphics device");
                return;
            }
        }
    };
}

/// What the option at `index`, counted from one, says.
fn label(index: usize) -> String {
    format!("Option {index:02}")
}

/// What it reports.
fn value(index: usize) -> String {
    format!("o{index:02}")
}

/// The smallest laid-out box that holds the first and the last option, which is the list.
fn list(stage: &Stage) -> Rect<DevicePx, Device> {
    let (first, last) = (label(1), label(COUNT));
    stage
        .census()
        .nodes
        .into_iter()
        .filter(|node| node.text.contains(&first) && node.text.contains(&last) && node.area() > 0.0)
        .min_by(|left, right| left.area().total_cmp(&right.area()))
        .and_then(|node| node.rect)
        .expect("the list is open")
}

/// Whether `inner` lies inside `outer`, give or take a pixel.
fn within(outer: Rect<DevicePx, Device>, inner: Rect<DevicePx, Device>) -> bool {
    inner.origin.y.0 >= outer.origin.y.0 - 1.0
        && inner.origin.y.0 + inner.size.height.0 <= outer.origin.y.0 + outer.size.height.0 + 1.0
}

/// Asserts that the option at `index` is inside the list's box.
fn assert_shown(stage: &Stage, index: usize, after: &str) {
    let list = list(stage);
    // The smallest box that says the label, leaving out the focused control: a select's trigger
    // shows the chosen option's label too.
    let control = stage.focused().map(|focused| stage.rect_of(focused));
    let text = label(index);
    let option = stage
        .census()
        .nodes
        .into_iter()
        .filter(|node| node.text == text && node.area() > 0.0)
        .filter_map(|node| node.rect)
        .filter(|rect| control.is_none_or(|control| !within(control, *rect)))
        .min_by(|left, right| {
            (left.size.width.0 * left.size.height.0)
                .total_cmp(&(right.size.width.0 * right.size.height.0))
        })
        .unwrap_or_else(|| panic!("{text} is laid out"));
    assert!(
        list.size.height.0 < (COUNT as f32) * 20.0,
        "the list is capped, so some options are past the fold: {list:?}"
    );
    assert!(
        within(list, option),
        "after {after}, {} is at {option:?}, outside the list at {list:?}",
        label(index)
    );
}

/// Asserts that the focused element is inside the list's box.
fn assert_focus_shown(stage: &Stage, after: &str) {
    let list = list(stage);
    let focused = stage.focused().expect("something has focus");
    let option = stage.rect_of(focused);
    assert!(
        within(list, option),
        "after {after}, the focused item is at {option:?}, outside the list at {list:?}"
    );
}

/// Presses `key` `times` times.
fn press(stage: &mut Stage, key: NamedKey, times: usize) {
    for _ in 0..times {
        stage.press_named(key);
    }
    stage.wait(SETTLED);
}

/// A select over thirty options, with `chosen` chosen.
fn select(chosen: usize) -> impl Fn() -> AnyView {
    move || {
        AnyView::new(view! {
            ThemeProvider {
                column(class = "page") {
                    Select(default_value = value(chosen)) {
                        SelectTrigger(label = "Option") {SelectValue(placeholder = "Choose one")}
                        SelectContent {
                            {(1..=COUNT)
                                .map(|index| AnyView::new(view! {
                                    SelectItem(value = value(index)) {{label(index)}}
                                }))
                                .collect::<Vec<_>>()}
                        }
                    }
                }
            }
        })
    }
}

#[test]
fn a_select_opens_with_a_far_chosen_option_in_view() {
    let mut stage = staged!(select(25));
    stage.press_named(NamedKey::Tab);
    press(&mut stage, NamedKey::ArrowDown, 1);
    assert_shown(&stage, 25, "opening on the chosen option");
}

#[test]
fn end_and_home_keep_the_active_option_of_a_select_in_view() {
    let mut stage = staged!(select(1));
    stage.press_named(NamedKey::Tab);
    press(&mut stage, NamedKey::ArrowDown, 1);
    press(&mut stage, NamedKey::End, 1);
    assert_shown(&stage, COUNT, "End");
    press(&mut stage, NamedKey::Home, 1);
    assert_shown(&stage, 1, "Home");
}

#[test]
fn the_down_arrow_keeps_the_active_option_of_a_combobox_in_view() {
    let mut stage = staged!(|| AnyView::new(view! {
        ThemeProvider {
            column(class = "page") {
                Combobox {
                    ComboboxInput(placeholder = "Search", label = "Option")
                    ComboboxContent {
                        {(1..=COUNT)
                            .map(|index| AnyView::new(view! {
                                ComboboxItem(value = value(index), text = label(index)) {
                                    {label(index)}
                                }
                            }))
                            .collect::<Vec<_>>()}
                    }
                }
            }
        }
    }));
    stage.press_named(NamedKey::Tab);
    press(&mut stage, NamedKey::ArrowDown, 1);
    press(&mut stage, NamedKey::ArrowDown, 20);
    assert_shown(&stage, 21, "twenty presses of the down arrow");
}

#[test]
fn the_down_arrow_keeps_the_active_command_in_view() {
    let mut stage = staged!(|| AnyView::new(view! {
        ThemeProvider {
            column(class = "page") {
                Command {
                    CommandInput(placeholder = "Type a command…", label = "Command")
                    CommandList {
                        {(1..=COUNT)
                            .map(|index| AnyView::new(view! {
                                CommandItem(value = value(index), text = label(index)) {
                                    {label(index)}
                                }
                            }))
                            .collect::<Vec<_>>()}
                    }
                }
            }
        }
    }));
    stage.press_named(NamedKey::Tab);
    press(&mut stage, NamedKey::ArrowDown, 20);
    assert_shown(&stage, 21, "twenty presses of the down arrow");
}

#[test]
fn the_down_arrow_keeps_the_focused_menu_item_in_view() {
    let mut stage = staged!(|| AnyView::new(view! {
        ThemeProvider {
            column(class = "page") {
                DropdownMenu {
                    DropdownMenuTrigger {"Open"}
                    DropdownMenuContent {
                        {(1..=COUNT)
                            .map(|index| AnyView::new(view! { MenuItem {{label(index)}} }))
                            .collect::<Vec<_>>()}
                    }
                }
            }
        }
    }));
    stage.press_named(NamedKey::Tab);
    press(&mut stage, NamedKey::Enter, 1);
    press(&mut stage, NamedKey::ArrowDown, 25);
    assert_focus_shown(&stage, "twenty-five presses of the down arrow");
    press(&mut stage, NamedKey::Home, 1);
    assert_focus_shown(&stage, "Home");
    press(&mut stage, NamedKey::End, 1);
    assert_focus_shown(&stage, "End");
}
