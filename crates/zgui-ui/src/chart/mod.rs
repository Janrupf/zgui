//! Numbers, drawn as shapes a reader can reach.

mod container;
mod geometry;
mod legend;
mod scale;
mod series;
mod style;
mod tooltip;

pub use crate::chart::container::{ChartContainer, ChartContainerProps};
pub use crate::chart::geometry::{
    MarkBox, Plot, area_path, axes_path, category_step, line_path, measured_gutter, rect_path,
    value_gutter,
};
pub use crate::chart::legend::{
    ChartLegendContent, ChartLegendContentProps, LegendAlign, LegendEntry,
};
pub use crate::chart::scale::{Scale, nice_step, tick_label};
pub use crate::chart::series::{ChartKind, Datum, Series};
pub use crate::chart::style::ChartStyle;
pub use crate::chart::tooltip::{
    ChartEntry, ChartIndicator, ChartTooltipContent, ChartTooltipContentProps,
};

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, Memo, RwSignal};
use zgui::view::CustomPropertyName;
use zgui::vocab::{PropValue, SharedString};
use zgui::{component, view};

use crate::chart::geometry::LABEL_GAP;

/// What the chart's rules are installed under.
pub(crate) const SHEET: &str = "zui-chart";

/// How wide a point mark is, in CSS pixels.
const POINT_SIZE: f64 = 8.0;

/// How far under the plot the category labels start, in CSS pixels.
const CATEGORY_GAP: f64 = 4.0;

/// A chart of one or more series, drawn as paths and readable as a list of numbers.
///
/// ```
/// use zgui::prelude::*;
/// use zgui::{component, view};
/// use zgui_ui::prelude::*;
///
/// /// How many of each were sold.
/// #[component]
/// fn Sales() -> impl IntoView {
///     let series = vec![Series::new(
///         "Units",
///         vec![
///             Datum::new("Jan", 120.0),
///             Datum::new("Feb", 180.0),
///             Datum::new("Mar", 90.0),
///         ],
///     )];
///     view! { Chart(series = series, label = "Units sold", kind = ChartKind::Bar) }
/// }
/// ```
///
/// # Every mark is an element
///
/// A bar is a `<vector>` of its own, with its own outline, its own paint and its own name — *Units,
/// Feb, 180* — so it is hoverable, focusable and read out. A chart drawn as one path would be one
/// element saying nothing, and a reader would meet a picture with a caption instead of the numbers
/// in it.
///
/// The axes and the grid go the other way: they are one path, because they are one thing to a
/// reader and because a dozen separate lines would be a dozen elements nobody can use.
///
/// # Axes
///
/// The value axis is labelled at its ticks, right-aligned against the axis. Its gutter is as wide
/// as the longest label once the labels are laid out, or `axis_width` when given. The category
/// axis is labelled under each band from the first series. When the bands are too narrow for every
/// label, only every second, third or later label is drawn.
///
/// # Several series
///
/// A bar chart puts the bars of one category side by side in its band, in series order. A line or
/// an area chart draws each series through the middle of the bands.
///
/// # Keyboard
///
/// Each mark is a tab stop, so <kbd>Tab</kbd> walks the data in the order it is drawn. Nothing
/// else: a chart is read rather than operated, and a chart that claimed the arrow keys would be one
/// nobody can scroll past.
///
/// # What the pointer summons
///
/// A [`ChartTooltipContent`] over the mark, naming the point and every series' value at it — one
/// card for the whole column rather than one per series, because "how did February go" is one
/// question. It is deaf to the pointer, so it cannot get between the pointer and the mark that
/// summoned it. [`ChartLegendContent`] draws the key under the plot.
///
/// # Colour
///
/// From the theme's five chart tokens, chosen by [`Series::tone`], reaching the shape through
/// `--zgui-fill` and `--zgui-stroke` — the properties this build paints vector content from. The
/// SVG paint longhands are not properties the style engine generates here, so `fill: red` on a
/// `<vector>` is discarded at parse; see the parity register.
#[component]
pub fn Chart(
    /// The series to draw.
    #[prop(into)]
    series: Signal<Vec<Series>, LocalStorage>,
    /// How to draw them.
    #[prop(default = ChartKind::Bar)]
    kind: ChartKind,
    /// How wide the chart is, in CSS pixels.
    #[prop(default = 480.0)]
    width: f64,
    /// How tall it is, in CSS pixels.
    #[prop(default = 240.0)]
    height: f64,
    /// Roughly how many values to label the value axis with.
    #[prop(default = 5)]
    ticks: usize,
    /// How wide the value axis' gutter is, in CSS pixels. By default it follows the longest label.
    #[prop(optional)]
    axis_width: Option<f64>,
    /// Whether to draw a key naming each series.
    #[prop(default = true)]
    legend: bool,
    /// Whether the pointer summons a card naming what it is over.
    #[prop(default = true)]
    tooltip: bool,
    /// What the swatch in that card is drawn as.
    #[prop(default = ChartIndicator::Dot)]
    indicator: ChartIndicator,
    /// What the chart is called, for a reader.
    #[prop(into, optional)]
    label: Option<String>,
    /// Where to record the chart's own element.
    #[prop(optional)]
    node_ref: Option<NodeRef>,
    /// Classes merged after the chart's own.
    #[prop(into, optional)]
    class: Classes,
    /// Anything else the caller forwarded.
    #[prop(attrs)]
    attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, ChartStyle::CSS);
    let element = node_ref.unwrap_or_default();
    // The value scale and its ticks, which change only when the data does. The scale reads the
    // plot's height alone, so it does not wait for the gutter.
    let axis = Memo::new(move |_| {
        let base = Plot::new(width, height);
        let scale = series.with(|runs| base.value_scale(runs.iter().flat_map(Series::values)));
        let marks = scale.ticks(ticks);
        (scale, marks)
    });
    // The widest value label as laid out, in CSS pixels. The probe is a 100px unit beside a column
    // of the labels, so the unit converts device pixels without asking for the window's scale.
    let probe = NodeRef::new();
    let unit = NodeRef::new();
    let probe_size = probe.observe_border_size_while(move || axis_width.is_none());
    let unit_size = unit.observe_border_size_while(move || axis_width.is_none());
    let widest = Memo::new(move |_| {
        let labels = f64::from(probe_size.get()?.width.0);
        let unit = f64::from(unit_size.get()?.width.0) / 100.0;
        (unit > 0.0).then(|| ((labels - 100.0 * unit) / unit).max(0.0).ceil())
    });
    // The plot with its gutter: the caller's width, else the measured labels, else an estimate
    // until the first measurement arrives.
    let layout = Memo::new(move |_| {
        let (scale, marks) = axis.get();
        let left = axis_width.unwrap_or_else(|| {
            widest
                .get()
                .map_or_else(|| value_gutter(&marks), measured_gutter)
        });
        (Plot::new(width, height).with_left(left), scale, marks)
    });
    // Which measurement the pointer is over, and where its mark is. Held rather than derived,
    // because it is an answer only the pointer knows.
    let over = RwSignal::new_local(None::<Over>);

    let mut semantics = A11yBinding::new(Role::Figure);
    if let Some(text) = label.clone() {
        semantics = semantics.label(text);
    }
    let own = Attrs::new()
        .class_toggle(zgui::view::ClassName::new("zui-chart"), true)
        .class_toggle(zgui::view::ClassName::new(ChartStyle::CLASS), true)
        .attribute(
            zgui::view::AttrName::new("data-kind"),
            Some(kind.name().to_owned()),
        )
        .custom_property(CustomPropertyName::new("zui-chart-width"), move || {
            Some(format!("{width}px"))
        })
        .custom_property(CustomPropertyName::new("zui-chart-height"), move || {
            Some(format!("{height}px"))
        })
        .a11y_from(semantics);

    view! {
        box(node_ref = element, {..own}, {..attrs}, class = class) {
            if move || axis_width.is_none() {
                row(class = "zui-chart__probe", a11y:hidden = true, node_ref = probe) {
                    box(class = "zui-chart__probe-unit", node_ref = unit) {}
                    column {
                        {move || axis.with(|(_, marks)| probe_labels(marks))}
                    }
                }
            } else {}
            box(class = "zui-chart__plot") {
                vector(
                    class = "zui-chart__axes",
                    a11y:hidden = true,
                    prop:d = move || {
                        let path = layout.with(|(plot, scale, marks)| axes_path(plot, scale, marks));
                        PropValue::from(SharedString::from(path))
                    }
                ) {}
                {move || {
                    let (plot, scale, marks) = layout.get();
                    value_labels(&plot, &scale, &marks)
                }}
                {move || {
                    let (plot, scale, _) = layout.get();
                    let runs = series.get();
                    let count = runs.len();
                    let axis = runs.first().map(|first| category_labels(&plot, first));
                    let mut views: Vec<AnyView> = runs
                        .into_iter()
                        .enumerate()
                        .map(|(index, run)| {
                            let over = tooltip.then_some(over);
                            AnyView::new(marks(plot, scale, kind, (index, count), run, over))
                        })
                        .collect();
                    views.extend(axis);
                    views
                }}
                {move || {
                    let here = over.get()?;
                    let runs = series.get();
                    let name = runs
                        .first()
                        .and_then(|run| run.points.get(here.point))
                        .map(|point| point.label.clone())
                        .unwrap_or_default();
                    let entries: Vec<ChartEntry> = runs
                        .iter()
                        .filter_map(|run| {
                            run.points.get(here.point).map(|point| {
                                ChartEntry::new(
                                    run.name.clone(),
                                    point.value,
                                    run.colour_token(),
                                )
                            })
                        })
                        .collect();
                    Some(AnyView::new(view! {
                        box(class = "zui-chart__readout", a11y:hidden = true, {..here.placed()}) {
                            ChartTooltipContent(
                                label = name,
                                entries = entries,
                                indicator = indicator
                            )
                        }
                    }))
                }}
            }
            if move || legend {
                {move || {
                    let entries: Vec<LegendEntry> = series
                        .get()
                        .iter()
                        .map(|run| LegendEntry::new(run.name.clone(), run.colour_token()))
                        .collect();
                    AnyView::new(view! { ChartLegendContent(entries = entries) })
                }}
            } else {}
        }
    }
}

/// Which measurement the pointer is over, and where on the plot its mark sits.
#[derive(Copy, Clone, PartialEq, Debug)]
struct Over {
    /// Which measurement of each series, counting from zero.
    point: usize,
    /// The middle of the mark, in plot pixels.
    x: f64,
    /// The top of the mark, in plot pixels.
    y: f64,
}

impl Over {
    /// The two custom properties that put the readout over the mark.
    fn placed(self) -> Attrs {
        let (x, y) = (self.x, self.y);
        Attrs::new()
            .custom_property(CustomPropertyName::new("zui-chart-readout-x"), move || {
                Some(format!("{x:.2}px"))
            })
            .custom_property(CustomPropertyName::new("zui-chart-readout-y"), move || {
                // Clear of the mark by a hair, so the card does not sit on the shape it names.
                Some(format!("{:.2}px", y - 8.0))
            })
    }
}

/// The four custom properties that put one mark's element where its geometry says it goes.
///
/// A mark is placed by its own box rather than by covering the plot, because a box is what a
/// pointer, a hit test and `:hover` all answer from: marks that all covered the plot would be
/// stacked, and only the last one drawn would ever be reachable.
fn placed_at(box_: MarkBox) -> Attrs {
    Attrs::new()
        .custom_property(CustomPropertyName::new("zui-chart-mark-x"), move || {
            Some(format!("{:.2}px", box_.x))
        })
        .custom_property(CustomPropertyName::new("zui-chart-mark-y"), move || {
            Some(format!("{:.2}px", box_.y))
        })
        .custom_property(CustomPropertyName::new("zui-chart-mark-width"), move || {
            Some(format!("{:.2}px", box_.width))
        })
        .custom_property(
            CustomPropertyName::new("zui-chart-mark-height"),
            move || Some(format!("{:.2}px", box_.height)),
        )
}

/// The value axis' labels: one per tick, right-aligned against the axis and centred on the tick's
/// grid line.
fn value_labels(plot: &Plot, scale: &Scale, ticks: &[f64]) -> Vec<AnyView> {
    let right = plot.width - plot.x0() + LABEL_GAP;
    ticks
        .iter()
        .map(|tick| {
            let y = scale.at(*tick);
            AnyView::new(view! {
                text(
                    class = "zui-chart__label",
                    attr:data-axis = "value",
                    a11y:hidden = true,
                    style:top = {Some(format!("{y:.2}px"))},
                    style:right = {Some(format!("{right:.2}px"))}
                ) {
                    {tick_label(*tick)}
                }
            })
        })
        .collect()
}

/// The value labels as the probe lays them out: one per row, at their natural width.
fn probe_labels(ticks: &[f64]) -> Vec<AnyView> {
    ticks
        .iter()
        .map(|tick| AnyView::new(view! { text {{tick_label(*tick)}} }))
        .collect()
}

/// The category axis' labels: one under the middle of each band, from `run`'s measurements.
fn category_labels(plot: &Plot, run: &Series) -> AnyView {
    let count = run.points.len();
    let longest = run
        .points
        .iter()
        .map(|point| point.label.chars().count())
        .max();
    let (_, band) = plot.band(0, count);
    let step = category_step(band, longest.unwrap_or(0));
    let top = plot.y1() + CATEGORY_GAP;
    let labels: Vec<AnyView> = run
        .points
        .iter()
        .enumerate()
        .step_by(step)
        .map(|(position, point)| {
            let (start, band) = plot.band(position, count);
            let x = start + band / 2.0;
            AnyView::new(view! {
                text(
                    class = "zui-chart__label",
                    attr:data-axis = "category",
                    a11y:hidden = true,
                    style:top = {Some(format!("{top:.2}px"))},
                    style:left = {Some(format!("{x:.2}px"))}
                ) {
                    {point.label.clone()}
                }
            })
        })
        .collect();
    AnyView::new(labels)
}

/// One series' marks: a shape per measurement, plus the connecting path when there is one.
///
/// `series` is the series' index and the number of series, which places its bars in each band.
fn marks(
    plot: Plot,
    scale: Scale,
    kind: ChartKind,
    series: (usize, usize),
    run: Series,
    over: Option<RwSignal<Option<Over>, LocalStorage>>,
) -> impl IntoView {
    let (index, series_count) = series;
    let tone = run.colour_token();
    let count = run.points.len();
    let name = run.name.clone();

    // Where each measurement sits, which the line, the area and the point marks all share.
    let placed: Vec<(f64, f64)> = run
        .points
        .iter()
        .enumerate()
        .map(|(position, point)| {
            let (start, band) = plot.band(position, count);
            (start + band / 2.0, scale.at(point.value))
        })
        .collect();

    let connector = match kind {
        ChartKind::Bar => String::new(),
        ChartKind::Line => line_path(&placed),
        ChartKind::Area => area_path(&placed, scale.at(0.0)),
    };
    let shape = match kind {
        ChartKind::Bar => "bar",
        ChartKind::Line => "line",
        ChartKind::Area => "area",
    };

    let marks: Vec<AnyView> = run
        .points
        .iter()
        .enumerate()
        .map(|(position, point)| {
            let (start, band) = plot.band(position, count);
            let placement = match kind {
                ChartKind::Bar => {
                    let (x, width) = plot.bar_slot(position, count, index, series_count);
                    MarkBox::bar(x, width, point.value, &scale)
                }
                ChartKind::Line | ChartKind::Area => {
                    let (x, y) = placed[position];
                    MarkBox::point(x, y, POINT_SIZE)
                }
            };
            let described = format!("{}, {}, {}", name, point.label, tick_label(point.value));
            // Over the middle of the band, which is where the readout for all its series belongs.
            let here = Over {
                point: position,
                x: start + band / 2.0,
                y: placement.y,
            };
            AnyView::new(view! {
                vector(
                    class = "zui-chart__mark",
                    tabindex = Focus::Sequential,
                    on:pointer_enter = move |_| if let Some(over) = over { over.set(Some(here)) },
                    on:pointer_leave = move |_| if let Some(over) = over { over.set(None) },
                    attr:data-shape = shape,
                    attr:data-series = {Some(index.to_string())},
                    attr:data-point = {Some(position.to_string())},
                    a11y:role = Role::Image,
                    a11y:label = {described},
                    var:--zui-chart-tone = {Some(tone.clone())},
                    {..placed_at(placement)},
                    prop:d = {PropValue::from(SharedString::from(placement.path()))}
                )
            })
        })
        .collect();

    // Built once rather than behind a `Show`: whether a kind has a connecting path is decided by
    // the kind, which does not change while the chart is mounted.
    let connector_tone = tone.clone();
    let joined = (!connector.is_empty()).then(|| {
        AnyView::new(view! {
            vector(
                class = "zui-chart__mark",
                a11y:hidden = true,
                attr:data-shape = {if kind == ChartKind::Area { "area" } else { "line" }},
                var:--zui-chart-tone = {Some(connector_tone)},
                {..placed_at(MarkBox::whole(&plot))},
                prop:d = {PropValue::from(SharedString::from(connector))}
            )
        })
    });

    view! {
        box(a11y:role = Role::Group, a11y:label = name.clone()) {
            {joined}
            {marks}
        }
    }
}
