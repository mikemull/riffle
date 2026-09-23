use leptos::prelude::*;
use wasm_bindgen::prelude::*;

use crate::types::{FilterState, SiteSummary};

// Everything DOM/WebGL/MapLibre-instance-lifecycle stays in JS; Rust only
// ever sends "set these markers" and receives "this site_no was clicked".
#[wasm_bindgen(module = "/public/js/map-interop.js")]
extern "C" {
    #[wasm_bindgen(js_name = initMap)]
    fn init_map(container_id: &str);

    #[wasm_bindgen(js_name = setSites)]
    fn set_sites(container_id: &str, sites_json: &str);

    #[wasm_bindgen(js_name = onSiteClick)]
    fn on_site_click(container_id: &str, callback: &Closure<dyn FnMut(String)>);
}

const MAP_CONTAINER_ID: &str = "site-map";

#[component]
pub fn SiteMap(sites: Vec<SiteSummary>, filter: RwSignal<FilterState>) -> impl IntoView {
    let node_ref = NodeRef::<leptos::html::Div>::new();

    // Runs once the div is actually in the DOM: initialize the map and wire
    // clicks back into the shared filter signal (the same one the filter
    // panel and query resource already react to -- clicking a marker is
    // just another way to set it).
    Effect::new(move |_| {
        if node_ref.get().is_some() {
            init_map(MAP_CONTAINER_ID);

            let callback: Closure<dyn FnMut(String)> = Closure::new(move |site_no: String| {
                filter.update(|f| f.sites = vec![site_no]);
            });
            on_site_click(MAP_CONTAINER_ID, &callback);
            // Leaked deliberately: this closure needs to live as long as the
            // map itself, which is the lifetime of the page.
            callback.forget();

            if let Ok(json) = serde_json::to_string(&sites) {
                set_sites(MAP_CONTAINER_ID, &json);
            }
        }
    });

    view! { <div id=MAP_CONTAINER_ID node_ref=node_ref style="height: 400px; width: 100%;"></div> }
}
