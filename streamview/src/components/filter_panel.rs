use leptos::prelude::*;

use crate::server_fns::site_metadata;
use crate::types::{FilterState, SiteMetadataInfo, SiteSummary};

fn date_only(timestamp: &str) -> String {
    timestamp.chars().take(10).collect()
}

fn find_metadata<'a>(list: &'a [SiteMetadataInfo], site_no: &str) -> Option<&'a SiteMetadataInfo> {
    list.iter().find(|m| m.site_no == site_no)
}

/// Reads/writes draft filter fields locally and only pushes them into the
/// shared `filter` signal on "Apply filters" -- avoids re-running a
/// DataFusion query on every keystroke.
#[component]
pub fn FilterPanel(sites: Vec<SiteSummary>, filter: RwSignal<FilterState>) -> impl IntoView {
    let initial = filter.get_untracked();
    let (selected_sites, set_selected_sites) = signal(initial.sites);
    let (param_cd, set_param_cd) = signal(initial.param_cd);
    let (start, set_start) = signal(initial.start.unwrap_or_default());
    let (end, set_end) = signal(initial.end.unwrap_or_default());

    // USGS descriptive metadata (name, location, period of record) -- a
    // separate, independently-loading enhancement on top of `sites`
    // (which only carries site_no/lat/lon, derived live from the lake
    // itself). Loads from a static file; see server_fns::query::site_metadata.
    // Fetched here rather than passed down from App so the checkboxes render
    // immediately from `sites` and names fill in a moment later, rather
    // than blocking the whole panel on this fetch.
    let metadata_resource = Resource::new(|| (), |_| site_metadata());

    // Keep the checkboxes in sync with `filter` even when something *other*
    // than this panel changes it -- e.g. clicking a marker on the map sets
    // `filter.sites` directly, and without this the checkboxes would drift
    // out of sync until Apply is clicked here (which would then silently
    // revert the map-driven selection back to the stale checkbox state).
    Effect::new(move |_| {
        set_selected_sites.set(filter.get().sites);
    });

    let apply = move |_| {
        let start_val = start.get();
        let end_val = end.get();
        filter.set(FilterState {
            sites: selected_sites.get(),
            param_cd: param_cd.get(),
            start: (!start_val.is_empty()).then_some(start_val),
            end: (!end_val.is_empty()).then_some(end_val),
        });
    };

    let toggle_site = move |site: String| {
        set_selected_sites.update(|sites| {
            if let Some(pos) = sites.iter().position(|s| s == &site) {
                sites.remove(pos);
            } else {
                sites.push(site);
            }
        });
    };

    view! {
        <div class="filter-panel">
            <fieldset>
                <legend>"Sites (none selected = all)"</legend>
                <div>
                    {sites
                        .into_iter()
                        .map(|site| {
                            let site_for_click = site.site_no.clone();
                            let site_for_check = site.site_no.clone();
                            let site_for_label = site.site_no.clone();
                            view! {
                                <label>
                                    <input
                                        type="checkbox"
                                        on:change=move |_| toggle_site(site_for_click.clone())
                                        checked=move || selected_sites.get().contains(&site_for_check)
                                    />
                                    {move || {
                                        let name = metadata_resource
                                            .get()
                                            .and_then(|r| r.ok())
                                            .and_then(|list| find_metadata(&list, &site_for_label).map(|m| m.name.clone()));
                                        match name {
                                            Some(n) if !n.is_empty() => format!("{n} ({site_for_label})"),
                                            _ => site_for_label.clone(),
                                        }
                                    }}
                                </label>
                            }
                        })
                        .collect_view()}
                </div>
            </fieldset>

            <label>
                "Parameter code "
                <input
                    type="text"
                    prop:value=move || param_cd.get()
                    on:input=move |ev| set_param_cd.set(event_target_value(&ev))
                />
            </label>

            <label>
                "Start "
                <input
                    type="date"
                    prop:value=move || start.get()
                    on:input=move |ev| set_start.set(event_target_value(&ev))
                />
            </label>

            <label>
                "End "
                <input
                    type="date"
                    prop:value=move || end.get()
                    on:input=move |ev| set_end.set(event_target_value(&ev))
                />
            </label>

            <button on:click=apply>"Apply filters"</button>

            <Suspense fallback=|| ()>
                {move || {
                    metadata_resource
                        .get()
                        .and_then(|r| r.ok())
                        .filter(|list| !list.is_empty())
                        .map(|list| {
                            view! {
                                <div class="site-metadata">
                                    <h3>"Site info"</h3>
                                    <ul>
                                        {list
                                            .into_iter()
                                            .map(|site| {
                                                let location = [&site.county_name, &site.state_name]
                                                    .into_iter()
                                                    .flatten()
                                                    .cloned()
                                                    .collect::<Vec<_>>()
                                                    .join(", ");
                                                view! {
                                                    <li>
                                                        <strong>{site.name.clone()}</strong> " (" {site.site_no.clone()} ")"
                                                        <br />
                                                        {(!location.is_empty()).then_some(location)}
                                                        {site
                                                            .drainage_area
                                                            .map(|da| format!(" | drainage area: {da} sq mi"))}
                                                        {site
                                                            .hydrologic_unit_code
                                                            .clone()
                                                            .map(|huc| format!(" | HUC {huc}"))}
                                                        <ul>
                                                            {site
                                                                .series
                                                                .into_iter()
                                                                .map(|s| {
                                                                    let stat = s.statistic_id.unwrap_or_else(|| "-".to_string());
                                                                    let begin = s.begin.map(|b| date_only(&b)).unwrap_or_default();
                                                                    let end = s.end.map(|e| date_only(&e)).unwrap_or_default();
                                                                    let status = if s.primary.is_some() { "Primary" } else { "Provisional" };
                                                                    view! {
                                                                        <li>
                                                                            {s.parameter_name} " (" {s.parameter_code} ") stat " {stat} "  "
                                                                            {begin} " .. " {end} "  [" {status} "]"
                                                                        </li>
                                                                    }
                                                                })
                                                                .collect_view()}
                                                        </ul>
                                                    </li>
                                                }
                                            })
                                            .collect_view()}
                                    </ul>
                                </div>
                            }
                        })
                }}
            </Suspense>
        </div>
    }
}
