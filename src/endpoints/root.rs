pub struct RootHandler;

impl RootHandler {
    pub async fn root_check() -> &'static str {
        "Welcome to the Data Collector API! This is the root endpoint."
    }
}
