use leptos::prelude::*;

use crate::server_fns::{list_published_versions, publish_dataset, verify_published_version};
use crate::types::{FilterState, ManifestSummary};

fn short_hash(hash: &str) -> String {
    hash.chars().take(12).collect()
}

#[component]
fn VersionRow(dataset_name: Signal<String>, version: ManifestSummary) -> impl IntoView {
    let (verify_result, set_verify_result) = signal(None::<String>);
    let dir_name = version.dir_name.clone();

    let verify = move |_| {
        let dataset_name = dataset_name.get_untracked();
        let dir_name = dir_name.clone();
        set_verify_result.set(Some("verifying...".to_string()));
        leptos::task::spawn_local(async move {
            let outcome = match verify_published_version(dataset_name, dir_name).await {
                Ok(report) if report.ok => format!("OK -- hash {} matches", short_hash(&report.actual_hash)),
                Ok(report) => format!(
                    "MISMATCH -- expected {} got {} (files: {})",
                    short_hash(&report.expected_hash),
                    short_hash(&report.actual_hash),
                    report.mismatched_files.join(", ")
                ),
                Err(e) => format!("error: {e}"),
            };
            set_verify_result.set(Some(outcome));
        });
    };

    let parent = version.parent_hash.as_deref().map(short_hash).unwrap_or_else(|| "(none -- first version)".to_string());

    view! {
        <li>
            <div>
                <strong>{version.dir_name.clone()}</strong> " -- " {version.row_count} " rows, "
                {version.sites.join(", ")} " (" {version.param_cd.clone()} "), "
                {version.start.clone()} ".." {version.end.clone()}
            </div>
            <div>"content_hash: " {short_hash(&version.content_hash)} " parent_hash: " {parent}</div>
            <div>"upstream as of: " {version.max_last_modified.clone()} " approval: " {format!("{:?}", version.approval_status_counts)}</div>
            <button on:click=verify>"Verify"</button>
            " " {move || verify_result.get()}
        </li>
    }
}

#[component]
pub fn PublishPanel(filter: RwSignal<FilterState>) -> impl IntoView {
    let (dataset_name, set_dataset_name) = signal(String::new());
    let (refresh_tick, set_refresh_tick) = signal(0u32);
    let (publish_status, set_publish_status) = signal(String::new());

    let dataset_name_signal = Signal::derive(move || dataset_name.get());

    let versions = Resource::new(
        move || (dataset_name.get(), refresh_tick.get()),
        |(name, _)| async move {
            if name.trim().is_empty() {
                Ok(Vec::new())
            } else {
                list_published_versions(name).await
            }
        },
    );

    let publish = move |_| {
        let name = dataset_name.get();
        if name.trim().is_empty() {
            set_publish_status.set("dataset name is required".to_string());
            return;
        }
        let current_filter = filter.get();
        set_publish_status.set("publishing...".to_string());
        leptos::task::spawn_local(async move {
            match publish_dataset(current_filter, name).await {
                Ok(manifest) => {
                    set_publish_status.set(format!(
                        "published {} as {} rows, content_hash {}",
                        manifest.dir_name,
                        manifest.row_count,
                        short_hash(&manifest.content_hash)
                    ));
                    set_refresh_tick.update(|t| *t += 1);
                }
                Err(e) => set_publish_status.set(format!("publish failed: {e}")),
            }
        });
    };

    view! {
        <div class="publish-panel">
            <h2>"Publish"</h2>
            <label>
                "Dataset name "
                <input
                    type="text"
                    prop:value=move || dataset_name.get()
                    on:input=move |ev| set_dataset_name.set(event_target_value(&ev))
                />
            </label>
            <button on:click=publish>"Publish current filter"</button>
            <p>{move || publish_status.get()}</p>

            <Suspense fallback=|| view! { <p>"Loading versions..."</p> }>
                {move || {
                    versions
                        .get()
                        .map(|result| match result {
                            Ok(vs) if vs.is_empty() => view! { <p>"No published versions yet for this dataset name."</p> }.into_any(),
                            Ok(vs) => {
                                view! {
                                    <ul>
                                        {vs
                                            .into_iter()
                                            .map(|v| view! { <VersionRow dataset_name=dataset_name_signal version=v /> })
                                            .collect_view()}
                                    </ul>
                                }
                                    .into_any()
                            }
                            Err(e) => view! { <p>"Error: " {e.to_string()}</p> }.into_any(),
                        })
                }}
            </Suspense>
        </div>
    }
}
