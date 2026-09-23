use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, Utc};
use leptos::prelude::*;
use leptos_chartistry::*;

use crate::types::SiteReadingRow;

#[derive(Clone)]
struct ChartPoint {
    x: DateTime<Utc>,
    value: f64,
}

/// `datetime` is either a plain date ("2020-01-01", daily service) or a full
/// ISO 8601 timestamp ("2020-01-01T00:00:00+00:00", continuous service);
/// taking the first 10 characters handles both.
fn parse_date(datetime: &str) -> Option<DateTime<Utc>> {
    let date_part = &datetime[..10.min(datetime.len())];
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc())
}

/// One small chart per site, rather than one multi-line chart. Chartistry's
/// `Series`/`Line` API extracts each line from a fixed struct field via a
/// closure, which fits a known, fixed set of series -- not a dynamic,
/// filter-selected number of sites. Small multiples sidestep that mismatch.
#[component]
pub fn TimeSeriesChart(rows: Vec<SiteReadingRow>) -> impl IntoView {
    let mut by_site: BTreeMap<String, Vec<ChartPoint>> = BTreeMap::new();
    for row in &rows {
        if let Some(x) = parse_date(&row.datetime) {
            by_site.entry(row.site_no.clone()).or_default().push(ChartPoint { x, value: row.value });
        }
    }
    for points in by_site.values_mut() {
        points.sort_by_key(|p| p.x);
    }

    view! {
        <div class="charts">
            {by_site
                .into_iter()
                .map(|(site_no, points)| {
                    let data: Signal<Vec<ChartPoint>> = Signal::derive(move || points.clone());
                    view! {
                        <div class="chart">
                            <Chart
                                aspect_ratio=AspectRatio::from_outer_ratio(600.0, 250.0)
                                top=RotatedLabel::middle(site_no.clone())
                                left=TickLabels::aligned_floats()
                                bottom=TickLabels::timestamps()
                                inner=[
                                    AxisMarker::left_edge().into_inner(),
                                    AxisMarker::bottom_edge().into_inner(),
                                    XGridLine::default().into_inner(),
                                    YGridLine::default().into_inner(),
                                    XGuideLine::over_data().into_inner(),
                                    YGuideLine::over_mouse().into_inner(),
                                ]
                                tooltip=Tooltip::left_cursor()
                                series=Series::new(|d: &ChartPoint| d.x)
                                    .line(Line::new(|d: &ChartPoint| d.value).with_name("value"))
                                data=data
                            />
                        </div>
                    }
                })
                .collect_view()}
        </div>
    }
}
