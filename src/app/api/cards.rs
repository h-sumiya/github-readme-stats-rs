use std::collections::HashMap;

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::Response;
use tracing::error;

use super::super::*;

pub(super) async fn stats_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response<Body> {
    handle_svg_cached(state, "/api", params, |state, params| async move {
        let render_options = render_options_from_query(&params);
        let username = q(&params, "username");

        if let Some(svg) = guard_access(
            &state.config,
            username,
            AccessType::Username,
            &render_options,
        ) {
            return svg_error_payload(svg, &state.config);
        }

        let show = parse_csv_param(&params, "show");
        let include_merged = show
            .iter()
            .any(|x| x == "prs_merged" || x == "prs_merged_percentage");
        let include_discussions = show.iter().any(|x| x == "discussions_started");
        let include_discussions_answers = show.iter().any(|x| x == "discussions_answered");

        match fetch_stats(
            &state,
            username.unwrap_or_default(),
            parse_bool_param(&params, "include_all_commits").unwrap_or(false),
            parse_csv_param(&params, "exclude_repo"),
            include_merged,
            include_discussions,
            include_discussions_answers,
            parse_i64_param(&params, "commits_year"),
        )
        .await
        {
            Ok(stats) => {
                let cache_seconds = resolve_cache_seconds(
                    parse_i64_param(&params, "cache_seconds"),
                    86_400,
                    43_200,
                    172_800,
                    &state.config,
                );

                let svg = render_stats_card(&stats, &params, &render_options);
                svg_success_payload(svg, cache_seconds, &state.config)
            }
            Err(err) => {
                error!("stats error: {err}");
                let svg = render_error_card(
                    &err.to_string(),
                    err.secondary_message(),
                    &render_options,
                    err.show_repo_link(),
                );
                svg_error_payload(svg, &state.config)
            }
        }
    })
    .await
}

pub(super) async fn top_langs_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response<Body> {
    handle_svg_cached(
        state,
        "/api/top-langs",
        params,
        |state, params| async move {
            let render_options = render_options_from_query(&params);
            let username = q(&params, "username");

            if let Some(svg) = guard_access(
                &state.config,
                username,
                AccessType::Username,
                &render_options,
            ) {
                return svg_error_payload(svg, &state.config);
            }

            if let Some(layout) = q(&params, "layout") {
                let valid = ["compact", "normal", "donut", "donut-vertical", "pie"];
                if !valid.contains(&layout) {
                    let svg = render_error_card(
                        "Something went wrong",
                        Some("Incorrect layout input"),
                        &render_options,
                        false,
                    );
                    return svg_error_payload(svg, &state.config);
                }
            }

            if let Some(stats_format) = q(&params, "stats_format") {
                let valid = ["bytes", "percentages"];
                if !valid.contains(&stats_format) {
                    let svg = render_error_card(
                        "Something went wrong",
                        Some("Incorrect stats_format input"),
                        &render_options,
                        false,
                    );
                    return svg_error_payload(svg, &state.config);
                }
            }

            match fetch_top_languages(
                &state,
                username.unwrap_or_default(),
                parse_csv_param(&params, "exclude_repo"),
                parse_f64_param(&params, "size_weight").unwrap_or(1.0),
                parse_f64_param(&params, "count_weight").unwrap_or(0.0),
            )
            .await
            {
                Ok(top_langs) => {
                    let cache_seconds = resolve_cache_seconds(
                        parse_i64_param(&params, "cache_seconds"),
                        518_400,
                        172_800,
                        864_000,
                        &state.config,
                    );
                    let svg = render_top_languages_card(&top_langs, &params, &render_options);
                    svg_success_payload(svg, cache_seconds, &state.config)
                }
                Err(err) => {
                    error!("top-langs error: {err}");
                    let svg = render_error_card(
                        &err.to_string(),
                        err.secondary_message(),
                        &render_options,
                        err.show_repo_link(),
                    );
                    svg_error_payload(svg, &state.config)
                }
            }
        },
    )
    .await
}

pub(super) async fn pin_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response<Body> {
    handle_svg_cached(state, "/api/pin", params, |state, params| async move {
        let render_options = render_options_from_query(&params);
        let username = q(&params, "username");

        if let Some(svg) = guard_access(
            &state.config,
            username,
            AccessType::Username,
            &render_options,
        ) {
            return svg_error_payload(svg, &state.config);
        }

        match fetch_repo(
            &state,
            username.unwrap_or_default(),
            q(&params, "repo").unwrap_or_default(),
        )
        .await
        {
            Ok(repo) => {
                let cache_seconds = resolve_cache_seconds(
                    parse_i64_param(&params, "cache_seconds"),
                    864_000,
                    86_400,
                    864_000,
                    &state.config,
                );
                let svg = render_repo_card(&repo, &params, &render_options);
                svg_success_payload(svg, cache_seconds, &state.config)
            }
            Err(err) => {
                error!("pin error: {err}");
                let svg = render_error_card(
                    &err.to_string(),
                    err.secondary_message(),
                    &render_options,
                    err.show_repo_link(),
                );
                svg_error_payload(svg, &state.config)
            }
        }
    })
    .await
}

pub(super) async fn gist_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response<Body> {
    handle_svg_cached(state, "/api/gist", params, |state, params| async move {
        let render_options = render_options_from_query(&params);
        let id = q(&params, "id");

        if let Some(svg) = guard_access(&state.config, id, AccessType::Gist, &render_options) {
            return svg_error_payload(svg, &state.config);
        }

        match fetch_gist(&state, id.unwrap_or_default()).await {
            Ok(gist) => {
                let cache_seconds = resolve_cache_seconds(
                    parse_i64_param(&params, "cache_seconds"),
                    172_800,
                    86_400,
                    864_000,
                    &state.config,
                );
                let svg = render_gist_card(&gist, &params, &render_options);
                svg_success_payload(svg, cache_seconds, &state.config)
            }
            Err(err) => {
                error!("gist error: {err}");
                let svg = render_error_card(
                    &err.to_string(),
                    err.secondary_message(),
                    &render_options,
                    err.show_repo_link(),
                );
                svg_error_payload(svg, &state.config)
            }
        }
    })
    .await
}

pub(super) async fn wakatime_handler(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response<Body> {
    handle_svg_cached(state, "/api/wakatime", params, |state, params| async move {
        let render_options = render_options_from_query(&params);
        let username = q(&params, "username");

        if let Some(svg) = guard_access(
            &state.config,
            username,
            AccessType::Wakatime,
            &render_options,
        ) {
            return svg_error_payload(svg, &state.config);
        }

        match fetch_wakatime_stats(
            &state,
            username.unwrap_or_default(),
            q(&params, "api_domain"),
        )
        .await
        {
            Ok(data) => {
                let cache_seconds = resolve_cache_seconds(
                    parse_i64_param(&params, "cache_seconds"),
                    86_400,
                    43_200,
                    172_800,
                    &state.config,
                );
                let svg = render_wakatime_card(&data, &params, &render_options);
                svg_success_payload(svg, cache_seconds, &state.config)
            }
            Err(err) => {
                error!("wakatime error: {err}");
                let svg = render_error_card(
                    &err.to_string(),
                    err.secondary_message(),
                    &render_options,
                    err.show_repo_link(),
                );
                svg_error_payload(svg, &state.config)
            }
        }
    })
    .await
}
