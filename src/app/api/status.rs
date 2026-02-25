use std::collections::HashMap;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{Response, StatusCode};
use serde_json::json;

use super::super::*;

pub(super) async fn up_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response<Body> {
    handle_json_cached(
        state,
        "/api/status/up",
        params,
        |state, params| async move {
            let up = check_pat_uptime(&state).await;
            let kind = q(&params, "type")
                .map(|v| v.to_ascii_lowercase())
                .unwrap_or_else(|| "boolean".to_string());

            let body = match kind.as_str() {
                "shields" => json!({
                    "schemaVersion": 1,
                    "label": "Public Instance",
                    "message": if up { "up" } else { "down" },
                    "color": if up { "brightgreen" } else { "red" },
                    "isError": true,
                })
                .to_string(),
                "json" => json!({ "up": up }).to_string(),
                _ => serde_json::to_string(&up).unwrap_or_else(|_| "false".to_string()),
            };

            let cache_headers = if up {
                CacheHeaders::Control("max-age=0, s-maxage=300".to_string())
            } else {
                CacheHeaders::NoCache
            };

            ResponsePayload {
                status: StatusCode::OK,
                content_type: "application/json",
                body,
                cache_headers: cache_headers.clone(),
                internal_ttl: if matches!(cache_headers, CacheHeaders::NoCache) {
                    Duration::from_secs(0)
                } else {
                    Duration::from_secs(300)
                },
            }
        },
    )
    .await
}

pub(super) async fn pat_info_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response<Body> {
    handle_json_cached(
        state,
        "/api/status/pat-info",
        params,
        |state, _params| async move {
            let info = get_pat_info(&state).await;
            let body = serde_json::to_string_pretty(&info).unwrap_or_else(|_| "{}".to_string());
            ResponsePayload {
                status: StatusCode::OK,
                content_type: "application/json",
                body,
                cache_headers: CacheHeaders::Control("max-age=0, s-maxage=300".to_string()),
                internal_ttl: Duration::from_secs(300),
            }
        },
    )
    .await
}
