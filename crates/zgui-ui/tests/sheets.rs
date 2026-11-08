//! Where the default sheets put the parts of a component, read off a laid-out window.
//!
//! Each fixture mounts one component under the default theme and reads the boxes the layout gave
//! its parts. Nothing here is about pixels, so no graphics device is needed.

mod desktop;

use zgui::geom::{Device, DevicePx, Rect};
use zgui::view;
use zgui::view::NodeRef;
use zgui_ui::chart::{LegendEntry, Plot, measured_gutter, tick_label};
use zgui_ui::prelude::*;
use zgui_ui_icons::prelude::*;
use zgui_ui_icons::set::status::CHECK_CIRCLE;
use zgui_ui_tokens::prelude::*;

use crate::desktop::census::absolute;
use crate::desktop::stage::Stage;

/// The page every fixture is laid out on.
const SHEET: &str = ":root { font-family: sans-serif } .page { padding: 24px; width: 640px }";

/// How far apart two edges can be and still be the same edge.
const SLACK: f32 = 1.0;

/// The box of the innermost laid-out node that says exactly `text`.
fn text_box(stage: &Stage, text: &str) -> Rect<DevicePx, Device> {
    stage
        .census()
        .control(text)
        .and_then(|seen| seen.rect)
        .unwrap_or_else(|| panic!("nothing on the page says {text:?}"))
}

/// The box of the element `node` was bound to.
fn own_box(stage: &Stage, node: NodeRef) -> Rect<DevicePx, Device> {
    let id = node.get_untracked().expect("the element is in the tree");
    absolute(stage.handles(), id).expect("the element was laid out")
}

fn right(rect: Rect<DevicePx, Device>) -> f32 {
    rect.origin.x.0 + rect.size.width.0
}

fn bottom(rect: Rect<DevicePx, Device>) -> f32 {
    rect.origin.y.0 + rect.size.height.0
}

#[derive(Clone)]
struct Payment {
    email: &'static str,
    amount: &'static str,
}

fn payments() -> Vec<Payment> {
    vec![
        Payment {
            email: "ken@example.com",
            amount: "$316.00",
        },
        Payment {
            email: "abe@example.com",
            amount: "$242.00",
        },
    ]
}

fn payment_columns() -> Vec<Column<Payment>> {
    vec![
        Column::new("email", "Email", |p: &Payment| p.email.to_owned()),
        Column::new("amount", "Amount", |p: &Payment| p.amount.to_owned())
            .aligned(CellAlign::End)
            .sortable_by(|a: &Payment, b: &Payment| a.amount.cmp(b.amount)),
        Column::new("note", "Note", |_: &Payment| String::from("ok")).aligned(CellAlign::End),
    ]
}

#[test]
fn an_end_aligned_heading_lines_up_with_its_cells() {
    let mut stage = Stage::open(SHEET, || {
        view! {
            ThemeProvider(scheme = ColorScheme::Light) {
                box(class = "page") {
                    DataTable(
                        rows = payments(),
                        columns = payment_columns(),
                        row_id = |p: &Payment| p.email.to_owned()
                    )
                }
            }
        }
    });
    stage.settle();

    let cell = right(text_box(&stage, "$316.00"));
    let sortable = right(text_box(&stage, "Amount"));
    assert!(
        (sortable - cell).abs() <= SLACK,
        "the sortable heading ends at {sortable} and its cells at {cell}"
    );
    let cell = right(text_box(&stage, "ok"));
    let plain = right(text_box(&stage, "Note"));
    assert!(
        (plain - cell).abs() <= SLACK,
        "the plain heading ends at {plain} and its cells at {cell}"
    );
}

#[test]
fn a_virtualised_table_draws_rows_as_tall_as_its_row_size() {
    let mut stage = Stage::open(SHEET, || {
        view! {
            ThemeProvider(scheme = ColorScheme::Light) {
                box(class = "page") {
                    DataTable(
                        rows = payments(),
                        columns = payment_columns(),
                        row_id = |p: &Payment| p.email.to_owned(),
                        virtualized = true,
                        body_height = 300.0
                    )
                }
            }
        }
    });
    stage.settle();

    let first = text_box(&stage, "ken@example.com");
    let second = text_box(&stage, "abe@example.com");
    let step = second.origin.y.0 - first.origin.y.0;
    assert!(
        (step - 40.0).abs() <= SLACK,
        "rows are {step}px apart and the default row size is 40px"
    );
}

#[test]
fn an_icon_passed_to_an_alert_sits_beside_the_title() {
    let mut stage = Stage::open(SHEET, || {
        view! {
            ThemeProvider(scheme = ColorScheme::Light) {
                box(class = "page") {
                    Alert(live = false, icon = false) {
                        Icon(icon = CHECK_CIRCLE)
                        AlertTitle {"Saved"}
                        AlertDescription {"Your changes are kept."}
                    }
                    Alert(live = false) {
                        AlertTitle {"Built in"}
                    }
                }
            }
        }
    });
    stage.settle();

    let own = text_box(&stage, "Saved");
    let description = text_box(&stage, "Your changes are kept.");
    let built_in = text_box(&stage, "Built in");
    assert!(
        (own.origin.x.0 - built_in.origin.x.0).abs() <= SLACK,
        "a title after a child icon starts at {} and one after the built-in icon at {}",
        own.origin.x.0,
        built_in.origin.x.0
    );
    assert!(
        (own.origin.x.0 - description.origin.x.0).abs() <= SLACK,
        "the title and the description do not line up"
    );
}

#[test]
fn a_sidebar_separator_is_inset_and_does_not_widen_the_content() {
    let mut stage = Stage::open(SHEET, move || {
        view! {
            ThemeProvider(scheme = ColorScheme::Light) {
                SidebarProvider(shortcut = false) {
                    Sidebar(label = "Nav") {
                        SidebarContent {
                            SidebarGroup { SidebarGroupLabel {"A"} }
                            SidebarSeparator()
                            SidebarGroup { SidebarGroupLabel {"B"} }
                        }
                    }
                    SidebarInset { text {"Main"} }
                }
            }
        }
    });
    stage.settle();

    // The smallest box holding both groups is the content.
    let census = stage.census();
    let content = census.control("AB").expect("the content is laid out");
    let panel = content.rect.expect("the content has a box");
    let line = census
        .nodes
        .iter()
        .filter(|seen| seen.id != content.id && stage.handles().host.contains(content.id, seen.id))
        .filter_map(|seen| seen.rect)
        .find(|rect| (rect.size.height.0 - 1.0).abs() < 0.01)
        .expect("the separator is a one-pixel line in the content");
    assert!(
        (line.origin.x.0 - panel.origin.x.0 - 8.0).abs() <= SLACK
            && (right(panel) - right(line) - 8.0).abs() <= SLACK,
        "the separator spans {}..{} in content {}..{}, and is meant to be inset 8px each side",
        line.origin.x.0,
        right(line),
        panel.origin.x.0,
        right(panel)
    );
}

#[test]
fn a_navigation_menu_keeps_its_own_height_in_a_column() {
    let menu = NodeRef::new();
    let mut stage = Stage::open(SHEET, move || {
        view! {
            ThemeProvider(scheme = ColorScheme::Light) {
                column(style:min-height = "400px") {
                    Button {"Above"}
                    NavigationMenu(label = "Main", node_ref = menu) {
                        NavigationMenuList {
                            NavigationMenuItem(value = "a") {
                                NavigationMenuTrigger {"Section"}
                                NavigationMenuContent { NavigationMenuLink {"x"} }
                            }
                        }
                    }
                }
            }
        }
    });
    stage.settle();

    let root = own_box(&stage, menu);
    let trigger = text_box(&stage, "Section");
    assert!(
        root.size.height.0 < 60.0 && bottom(root) >= bottom(trigger),
        "the menu is {}px tall in a 400px column",
        root.size.height.0
    );
}

#[test]
fn a_leading_icon_before_a_bare_label_keeps_the_full_gap() {
    let bare = NodeRef::new();
    let wrapped = NodeRef::new();
    let trailing = NodeRef::new();
    let mut stage = Stage::open(SHEET, move || {
        view! {
            ThemeProvider(scheme = ColorScheme::Light) {
                column(class = "page", style:align-items = "flex-start") {
                    Button(node_ref = bare) { Icon(icon = CHECK_CIRCLE) "Bare label" }
                    Button(node_ref = wrapped) { Icon(icon = CHECK_CIRCLE) text {"Wrapped label"} }
                    Button(node_ref = trailing) { text {"Trailing"} Icon(icon = CHECK_CIRCLE) }
                }
            }
        }
    });
    stage.settle();

    let inset = |button: NodeRef, label: &str| {
        text_box(&stage, label).origin.x.0 - own_box(&stage, button).origin.x.0
    };
    let bare_inset = inset(bare, "Bare label");
    let wrapped_inset = inset(wrapped, "Wrapped label");
    assert!(
        (bare_inset - wrapped_inset).abs() <= SLACK,
        "a bare label starts {bare_inset}px into its button and a wrapped one {wrapped_inset}px"
    );
    // The leading mark sits 12px in and is 16px wide; the label follows after an 8px gap.
    assert!(
        (bare_inset - 36.0).abs() <= SLACK,
        "a bare label starts {bare_inset}px into its button, and 36px is the leading inset"
    );
    let label = text_box(&stage, "Trailing");
    let button = own_box(&stage, trailing);
    let after = right(button) - right(label);
    assert!(
        (after - 36.0).abs() <= SLACK,
        "a label before a trailing mark ends {after}px from the button's end"
    );
}

/// The box of the drawn label that says exactly `text`. The chart also lays its value labels out in
/// a hidden probe one level deeper, which this passes by.
fn drawn_label(stage: &Stage, text: &str) -> Rect<DevicePx, Device> {
    stage
        .census()
        .nodes
        .iter()
        .filter(|node| node.text == text && node.area() > 0.0)
        .min_by_key(|node| node.depth)
        .and_then(|node| node.rect)
        .unwrap_or_else(|| panic!("no drawn label says {text:?}"))
}

/// The gutter the chart gave its value labels: room for the widest of them as drawn.
fn drawn_gutter(stage: &Stage, ticks: &[f64]) -> f64 {
    let widest = ticks
        .iter()
        .map(|tick| drawn_label(stage, &tick_label(*tick)).size.width.0)
        .fold(0.0_f32, f32::max);
    measured_gutter(f64::from(widest).ceil())
}

fn revenue() -> Vec<Series> {
    vec![Series::new(
        "Revenue",
        vec![
            Datum::new("Jan", 42_100.0),
            Datum::new("Feb", 77_800.0),
            Datum::new("Mar", 51_000.0),
        ],
    )]
}

#[test]
fn value_labels_sit_on_their_grid_lines_inside_the_gutter_under_large_type() {
    let chart = NodeRef::new();
    let mut stage = Stage::open(SHEET, move || {
        view! {
            ThemeProvider(scheme = ColorScheme::Light) {
                box(
                    class = "page",
                    var:--zui-type-size-xs = "15px",
                    var:--zui-type-leading-xs = "22px"
                ) {
                    Chart(series = revenue(), kind = ChartKind::Line, legend = false, node_ref = chart)
                }
            }
        }
    });
    stage.settle();

    let origin = own_box(&stage, chart);
    let plot = Plot::new(480.0, 240.0);
    let scale = plot.value_scale(revenue().iter().flat_map(Series::values));
    let ticks = scale.ticks(5);
    let x0 = origin.origin.x.0 + drawn_gutter(&stage, &ticks) as f32;
    for tick in ticks.iter().filter(|tick| **tick > 0.0) {
        let label = drawn_label(&stage, &tick_label(*tick));
        let line = origin.origin.y.0 + scale.at(*tick) as f32;
        let centre = label.origin.y.0 + label.size.height.0 / 2.0;
        assert!(
            (centre - line).abs() <= SLACK,
            "the label {tick} is centred at {centre} and its grid line is at {line}"
        );
        assert!(
            right(label) <= x0 - 4.0,
            "the label {tick} ends at {} and the axis is at {x0}",
            right(label)
        );
        assert!(
            label.origin.x.0 >= origin.origin.x.0 - SLACK,
            "the label {tick} starts left of the chart"
        );
    }
}

#[test]
fn each_category_is_labelled_under_the_middle_of_its_band() {
    let chart = NodeRef::new();
    let mut stage = Stage::open(SHEET, move || {
        view! {
            ThemeProvider(scheme = ColorScheme::Light) {
                box(class = "page") {
                    Chart(series = revenue(), legend = false, node_ref = chart)
                }
            }
        }
    });
    stage.settle();

    let origin = own_box(&stage, chart);
    let ticks = Plot::new(480.0, 240.0)
        .value_scale(revenue().iter().flat_map(Series::values))
        .ticks(5);
    let plot = Plot::new(480.0, 240.0).with_left(drawn_gutter(&stage, &ticks));
    for (position, name) in ["Jan", "Feb", "Mar"].into_iter().enumerate() {
        let label = text_box(&stage, name);
        let (start, band) = plot.band(position, 3);
        let middle = origin.origin.x.0 + (start + band / 2.0) as f32;
        let centre = label.origin.x.0 + label.size.width.0 / 2.0;
        assert!(
            (centre - middle).abs() <= SLACK,
            "{name} is centred at {centre} and its band at {middle}"
        );
        assert!(
            label.origin.y.0 >= origin.origin.y.0 + plot.y1() as f32,
            "{name} is not under the plot"
        );
    }
}

#[test]
fn a_key_drawn_outside_a_chart_spaces_its_entries() {
    let mut stage = Stage::open(SHEET, move || {
        view! {
            ThemeProvider(scheme = ColorScheme::Light) {
                box(class = "page") {
                    ChartContainer {
                        ChartLegendContent(entries = vec![
                            LegendEntry::new("Reads", "var(--zui-color-chart-1)"),
                            LegendEntry::new("Writes", "var(--zui-color-chart-4)"),
                        ])
                    }
                }
            }
        }
    });
    stage.settle();

    let reads = text_box(&stage, "Reads");
    let writes = text_box(&stage, "Writes");
    // A swatch and the gaps on either side of it are between the two names.
    let between = writes.origin.x.0 - right(reads);
    assert!(
        between >= 8.0 + 16.0,
        "the two names are {between}px apart, so the key has no swatches or gaps"
    );
}

/// The height of the one avatar in a row that does not stretch it, under `tokens`.
fn avatar_height(tokens: &'static str, size: AvatarSize) -> f32 {
    let row = NodeRef::new();
    let sheet = format!("{SHEET} .tokens {{ {tokens} }}");
    let mut stage = Stage::open(&sheet, move || {
        view! {
            ThemeProvider(scheme = ColorScheme::Light) {
                row(node_ref = row, class = "tokens", style:align-items = "flex-start") {
                    Avatar(size = size) { AvatarFallback {"MW"} }
                }
            }
        }
    });
    stage.settle();
    own_box(&stage, row).size.height.0
}

#[test]
fn an_avatar_grows_with_its_type_and_its_spacing() {
    assert!((avatar_height("", AvatarSize::Md) - 32.0).abs() <= SLACK);
    assert!((avatar_height("", AvatarSize::Sm) - 24.0).abs() <= SLACK);
    // Tight spacing with ordinary type: the type decides.
    let dense = "--zui-space-base: 3px; --zui-type-size-sm: 12px; --zui-type-size-xs: 11px";
    assert!((avatar_height(dense, AvatarSize::Md) - 12.0 * 16.0 / 7.0).abs() <= SLACK);
    assert!((avatar_height(dense, AvatarSize::Sm) - 22.0).abs() <= SLACK);
    // Roomy spacing: the space unit decides.
    let roomy = "--zui-space-base: 5px";
    assert!((avatar_height(roomy, AvatarSize::Md) - 40.0).abs() <= SLACK);
}
