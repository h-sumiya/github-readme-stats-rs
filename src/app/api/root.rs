use axum::body::Body;
use axum::http::Response;

pub(super) async fn handler() -> Response<Body> {
    super::super::plain_text_response("github-readme-stats-rs")
}
