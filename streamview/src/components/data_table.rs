use leptos::prelude::*;

use crate::types::SiteReadingRow;

#[component]
pub fn DataTable(rows: Vec<SiteReadingRow>) -> impl IntoView {
    let row_count = rows.len();

    view! {
        <div class="data-table">
            <p>{row_count} " rows"</p>
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
                    {rows
                        .into_iter()
                        .map(|r| {
                            view! {
                                <tr>
                                    <td>{r.site_no}</td>
                                    <td>{r.param_cd}</td>
                                    <td>{r.datetime}</td>
                                    <td>{r.value}</td>
                                    <td>{r.approval_status}</td>
                                </tr>
                            }
                        })
                        .collect_view()}
                </tbody>
            </table>
        </div>
    }
}
