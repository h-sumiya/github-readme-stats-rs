mod cards;
mod root;
mod status;

use axum::Router;
use axum::routing::get;

pub(super) fn router() -> Router<super::AppState> {
    Router::new()
        .route("/", get(root::handler))
        .route("/api", get(cards::stats_handler))
        .route("/api/top-langs", get(cards::top_langs_handler))
        .route("/api/pin", get(cards::pin_handler))
        .route("/api/gist", get(cards::gist_handler))
        .route("/api/wakatime", get(cards::wakatime_handler))
        .route("/api/status/up", get(status::up_handler))
        .route("/api/status/pat-info", get(status::pat_info_handler))
}
