use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct QueryParams {
    pub q: Option<String>,
    pub page: Option<i64>,
}
