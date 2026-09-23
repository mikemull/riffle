use leptos::prelude::*;

use crate::types::{FilterState, SiteSummary};

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
                            view! {
                                <label>
                                    <input
                                        type="checkbox"
                                        on:change=move |_| toggle_site(site_for_click.clone())
                                        checked=move || selected_sites.get().contains(&site_for_check)
                                    />
                                    {site.site_no.clone()}
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
        </div>
    }
}
