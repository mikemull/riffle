#[derive(Debug, Clone)]
pub struct SiteReading {
    pub site_no: String,
    pub param_cd: String,
    pub datetime: String,
    pub value: f64,
    pub qualifiers: String,
    pub latitude: f64,
    pub longitude: f64,
    pub approval_status: String,
    pub last_modified: String,
}
