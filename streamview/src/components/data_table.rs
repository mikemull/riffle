use leptos::prelude::*;

use crate::types::SiteReadingRow;

const PAGE_SIZE: usize = 100;

/// Client-side pagination over an already-fetched result set -- the data
/// itself (all of `rows`) already came over the wire in one response; the
/// problem this fixes is purely DOM size (58,000+ rows rendered as `<tr>`s
/// at once made the page ~1.5 million pixels tall). `StoredValue` holds the
/// full Vec without re-cloning it on every page change; only the current
/// page's slice gets built into `<tr>`s.
// The `disabled=move || (... >= ...)` parens below look redundant to rustc,
// but they're load-bearing for the `view!` macro's own parser: a bare `>=`
// in an attribute-value closure gets misparsed as tag-closing syntax,
// leaking literal source text into the rendered page and leaving the button
// permanently disabled (found live, via a real click test). Don't "clean up"
// the parens without re-testing.
#[component]
#[allow(unused_parens)]
pub fn DataTable(rows: Vec<SiteReadingRow>) -> impl IntoView {
    let row_count = rows.len();
    let total_pages = row_count.div_ceil(PAGE_SIZE).max(1);
    let rows = StoredValue::new(rows);

    let (page, set_page) = signal(0usize);

    view! {
        <div class="data-table">
            <p>{row_count} " rows"</p>
            <div class="data-table-pager">
                <button
                    on:click=move |_| set_page.update(|p| *p = p.saturating_sub(1))
                    disabled=move || page.get() == 0
                >
                    "Previous"
                </button>
                " Page " {move || page.get() + 1} " of " {total_pages} " "
                <button
                    on:click=move |_| set_page.update(|p| *p = (*p + 1).min(total_pages - 1))
                    disabled=move || (page.get() + 1 >= total_pages)
                >
                    "Next"
                </button>
            </div>
            <table>
                <thead>
                    <tr>
                        <th>"Site"</th>
                        <th>"Param"</th>
                        <th>"Date"</th>
                        <th>"Value"</th>
                        <th>"Approval"</th>
                    </tr>
                </thead>
                <tbody>
                    {move || {
                        let start = page.get() * PAGE_SIZE;
                        rows.with_value(|all| {
                            let end = (start + PAGE_SIZE).min(all.len());
                            all[start..end]
                                .iter()
                                .map(|r| {
                                    view! {
                                        <tr>
                                            <td>{r.site_no.clone()}</td>
                                            <td>{r.param_cd.clone()}</td>
                                            <td>{r.datetime.clone()}</td>
                                            <td>{r.value}</td>
                                            <td>{r.approval_status.clone()}</td>
                                        </tr>
                                    }
                                })
                                .collect_view()
                        })
                    }}
                </tbody>
            </table>
        </div>
    }
}
