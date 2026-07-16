#[utoipa::path(
    get,
    path = "/",
    responses(
        (status = 200, description = "Welcome message", body = String, example = json!("Welcome to the Data Collector API! This is the root endpoint."))
    ),
    tag = "General"
)]
pub async fn root_check() -> &'static str {
    "Welcome to the Data Collector API! This is the root endpoint."
}
