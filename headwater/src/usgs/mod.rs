pub mod client;
pub mod continuous;
pub mod daily;
pub mod model;

use crate::cli::Service;
use anyhow::Result;
use model::SiteReading;

pub fn fetch(
    service: Service,
    sites: &str,
    param: &str,
    start: &str,
    end: &str,
    api_key: Option<&str>,
) -> Result<Vec<SiteReading>> {
    match service {
        Service::Daily => daily::fetch(sites, param, start, end, api_key),
        Service::Continuous => continuous::fetch(sites, param, start, end, api_key),
    }
}
