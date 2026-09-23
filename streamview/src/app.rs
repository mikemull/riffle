use leptos::prelude::*;

use crate::components::{DataTable, FilterPanel, PublishPanel, SiteMap, TimeSeriesChart};
use crate::server_fns::{list_sites, query_readings};
use crate::types::FilterState;

#[component]
pub fn App() -> impl IntoView {
    let filter = RwSignal::new(FilterState::default());
    let sites_resource = Resource::new(|| (), |_| list_sites());
    let readings = Resource::new(move || filter.get(), query_readings);

    view! {
        <main>
            <h1>"streamview"</h1>
            <Suspense fallback=|| view! { <p>"Loading sites..."</p> }>
                {move || {
                    sites_resource
                        .get()
                        .map(|result| match result {
                            Ok(sites) => {
                                view! {
                                    <SiteMap sites=sites.clone() filter=filter />
                                    <FilterPanel sites=sites filter=filter />
                                }
                                    .into_any()
                            }
                            Err(e) => view! { <p>"Error loading sites: " {e.to_string()}</p> }.into_any(),
                        })
                }}
            </Suspense>
            <Suspense fallback=|| view! { <p>"Loading..."</p> }>
                {move || {
                    readings
                        .get()
                        .map(|result| match result {
                            Ok(rows) => {
                                view! {
                                    <TimeSeriesChart rows=rows.clone() />
                                    <DataTable rows=rows />
                                }
                                    .into_any()
                            }
                            Err(e) => view! { <p>"Error: " {e.to_string()}</p> }.into_any(),
                        })
                }}
            </Suspense>
            <PublishPanel filter=filter />
        </main>
    }
}
