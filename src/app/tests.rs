use super::*;
use axum::{
    Json, Router,
    extract::{Query, State},
    http::HeaderMap,
};
use std::collections::VecDeque;
use std::sync::Mutex;
use tokio::task::JoinHandle;

type Call = (String, HashMap<String, String>, Value, String);
#[derive(Default)]
struct MockData {
    replies: Mutex<VecDeque<(StatusCode, Value)>>,
    calls: Mutex<Vec<Call>>,
}
struct MockApi {
    state: AppState,
    data: Arc<MockData>,
    task: JoinHandle<()>,
}
impl Drop for MockApi {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn mock_handler(
    State(data): State<Arc<MockData>>,
    uri: axum::http::Uri,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    data.calls.lock().unwrap().push((
        uri.path().to_string(),
        query,
        serde_json::from_slice(&body).unwrap_or(Value::Null),
        headers
            .get("Authorization")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string(),
    ));
    let (status, reply) = data
        .replies
        .lock()
        .unwrap()
        .pop_front()
        .expect("unexpected GitHub request");
    (status, Json(reply))
}
async fn mock(replies: Vec<(StatusCode, Value)>) -> MockApi {
    let data = Arc::new(MockData::default());
    data.replies.lock().unwrap().extend(replies);
    let app = Router::new()
        .fallback(mock_handler)
        .with_state(data.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = Client::new();
    let state = AppState {
        config: Arc::new(AppConfig {
            host: "127.0.0.1".into(),
            port: 0,
            node_env: "production".into(),
            cache_seconds_override: None,
            fetch_multi_page_stars: 1,
            api_base_url: format!("http://{addr}"),
            tokens: vec![TokenEntry {
                name: "PAT_1".into(),
                token: "test-one".into(),
            }],
            exclude_repos: HashSet::new(),
            whitelist: None,
            gist_whitelist: None,
        }),
        github_client: client.clone(),
        http_client: client,
        cache: Arc::new(DashMap::new()),
        next_token: Arc::new(AtomicUsize::new(0)),
    };
    MockApi { state, data, task }
}
fn ok(v: Value) -> (StatusCode, Value) {
    (StatusCode::OK, v)
}
fn params(items: &[(&str, &str)]) -> HashMap<String, String> {
    items
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
fn stars(count: i64, cursor: Option<&str>) -> Value {
    json!({"nodes":[{"name":"project","stargazerCount":count}],"pageInfo":{"hasNextPage":cursor.is_some(),"endCursor":cursor}})
}
fn user(repos: Value) -> Value {
    json!({"data":{"user":{"name":"Hiro","login":"hiro","commits":{"totalCommitContributions":23},"reviews":{"totalPullRequestReviewContributions":4},
        "repositoriesContributedTo":{"totalCount":6},"pullRequests":{"totalCount":8},"openIssues":{"totalCount":2},"closedIssues":{"totalCount":3},
        "followers":{"totalCount":7},"repositories":repos}}})
}
fn repo_range(names: &[&str], own: bool) -> Value {
    let mut v = json!({"commitContributionsByRepository":names.iter().map(|name|json!({"repository":{"nameWithOwner":name}})).collect::<Vec<_>>(),
        "issueContributionsByRepository":[],"pullRequestContributionsByRepository":[]});
    if own {
        v["repositoryContributions"] =
            json!({"nodes":[{"repository":{"nameWithOwner":"Hiro/owned"}}]});
    }
    v
}

#[test]
fn page_limits_and_roles_preserve_default_and_accept_extended_options() {
    assert_eq!(parse_star_page_limit(None), 1);
    assert_eq!(parse_star_page_limit(Some("false")), 1);
    assert_eq!(parse_star_page_limit(Some("0")), 1);
    assert_eq!(parse_star_page_limit(Some("10")), 10);
    assert_eq!(parse_star_page_limit(Some("true")), usize::MAX);
    assert_eq!(
        owner_affiliations(&params(&[(
            "role",
            "owner,organization_member,collaborator"
        )]))
        .unwrap(),
        ["OWNER", "ORGANIZATION_MEMBER", "COLLABORATOR"]
    );
    assert!(owner_affiliations(&params(&[("role", "ADMIN")])).is_err());
}

#[tokio::test]
async fn stats_send_roles_and_owned_repository_option_and_sum_scalar_stars() {
    let api = mock(vec![ok(user(stars(5, None)))]).await;
    let p = params(&[
        ("username", "hiro"),
        ("role", "owner,collaborator"),
        ("contribs_include_own_repos", "true"),
    ]);
    let stats = fetch_stats(&api.state, "hiro", &p).await.unwrap();
    assert_eq!(
        (stats.total_stars, stats.total_commits, stats.total_issues),
        (5, 23, 5)
    );
    let calls = api.data.calls.lock().unwrap();
    let vars = &calls[0].2["variables"];
    assert_eq!(vars["ownerAffiliations"], json!(["OWNER", "COLLABORATOR"]));
    assert_eq!(vars["includeUserRepositories"], true);
    assert_eq!(vars["includeYears"], false);
}

#[tokio::test]
async fn star_pagination_respects_page_limit_and_stops_on_repeated_cursor() {
    for limit in [2, 10] {
        let mut api = mock(vec![
            ok(user(stars(5, Some("A")))),
            ok(json!({"data":{"user":{"repositories":stars(4,Some("A"))}}})),
        ])
        .await;
        Arc::make_mut(&mut api.state.config).fetch_multi_page_stars = limit;
        let stats = fetch_stats(&api.state, "hiro", &HashMap::new())
            .await
            .unwrap();
        assert_eq!(stats.total_stars, 9);
        assert_eq!(api.data.calls.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn star_pagination_stops_at_zero_stars_or_absent_cursor() {
    for repositories in [
        stars(0, Some("next")),
        json!({"nodes":[{"name":"p","stargazerCount":3}],"pageInfo":{"hasNextPage":true,"endCursor":null}}),
    ] {
        let mut api = mock(vec![ok(user(repositories))]).await;
        Arc::make_mut(&mut api.state.config).fetch_multi_page_stars = 10;
        fetch_stats(&api.state, "hiro", &HashMap::new())
            .await
            .unwrap();
        assert_eq!(api.data.calls.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn partial_graphql_errors_never_render_success_or_zero_stars() {
    let mut reply = user(stars(5, None));
    reply["errors"] = json!([{"type":"INTERNAL","message":"partial response"}]);
    let api = mock(vec![ok(reply)]).await;
    assert!(
        fetch_stats(&api.state, "hiro", &HashMap::new())
            .await
            .unwrap_err()
            .to_string()
            .contains("partial response")
    );
    let mut reply = user(stars(5, None));
    reply["data"]["user"]["repositories"]["nodes"][0]
        .as_object_mut()
        .unwrap()
        .remove("stargazerCount");
    let api = mock(vec![ok(reply)]).await;
    assert!(
        fetch_stats(&api.state, "hiro", &HashMap::new())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn http_errors_are_rejected_even_if_the_body_contains_data() {
    let api = mock(vec![(StatusCode::BAD_GATEWAY, user(stars(5, None)))]).await;
    assert!(
        fetch_stats(&api.state, "hiro", &HashMap::new())
            .await
            .unwrap_err()
            .to_string()
            .contains("502")
    );
}

#[tokio::test]
async fn rate_limited_partial_response_rotates_tokens_but_user_text_does_not() {
    let mut reply = user(stars(5, None));
    reply["errors"] = json!([{"type":"RATE_LIMITED","message":"rate limit"}]);
    let mut normal = user(stars(7, None));
    normal["data"]["user"]["name"] = json!("Bad credentials and rate limit research");
    let mut api = mock(vec![ok(reply), ok(normal)]).await;
    Arc::make_mut(&mut api.state.config)
        .tokens
        .push(TokenEntry {
            name: "PAT_2".into(),
            token: "test-two".into(),
        });
    assert_eq!(
        fetch_stats(&api.state, "hiro", &HashMap::new())
            .await
            .unwrap()
            .total_stars,
        7
    );
    let calls = api.data.calls.lock().unwrap();
    assert_eq!(calls[0].3, "Bearer test-one");
    assert_eq!(calls[1].3, "Bearer test-two");
}

#[tokio::test]
async fn transient_network_failure_retries_the_same_token() {
    // First TCP connection closes before HTTP; the next serves a valid response.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let api = mock(vec![ok(json!({"data":{"value":1}}))]).await;
    let data = api.data.clone();
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        drop(stream);
        axum::serve(
            listener,
            Router::new().fallback(mock_handler).with_state(data),
        )
        .await
        .unwrap();
    });
    let mut state = api.state.clone();
    Arc::make_mut(&mut state.config).api_base_url = format!("http://{addr}");
    assert_eq!(
        github_json(&state, "/graphql", None, Some(json!({})))
            .await
            .unwrap()["data"]["value"],
        1
    );
    assert_eq!(api.data.calls.lock().unwrap()[0].3, "Bearer test-one");
    task.abort();
}

#[tokio::test]
async fn all_year_contributions_are_summed_with_explicit_year_boundaries() {
    let api=mock(vec![ok(json!({"data":{"user":{"year_0":{"contributionCalendar":{"totalContributions":41}},"year_1":{"contributionCalendar":{"totalContributions":29}}}}}))]).await;
    let counts = fetch_all_time_stats(
        &api.state,
        "hiro",
        &[2023, 2024],
        &["contributions".into()],
        false,
    )
    .await
    .unwrap();
    assert_eq!(counts["contributions"], 70);
    let calls = api.data.calls.lock().unwrap();
    let query = calls[0].2["query"].as_str().unwrap();
    assert!(query.contains("2023-12-31T23:59:59Z"));
    assert!(query.contains("2024-01-01T00:00:00Z"));
}

#[tokio::test]
async fn all_time_repositories_are_deduplicated_across_years_and_own_repos_are_optional() {
    for own in [false, true] {
        let api=mock(vec![ok(json!({"data":{"user":{"range_0":repo_range(&["org/one","Hiro/owned"],own),"range_1":repo_range(&["ORG/ONE","org/two"],own)}}}))]).await;
        let counts = fetch_all_time_stats(
            &api.state,
            "hiro",
            &[2023, 2024],
            &["all_time_contribs".into()],
            own,
        )
        .await
        .unwrap();
        assert_eq!(counts["all_time_contribs"], if own { 3 } else { 2 });
        let calls = api.data.calls.lock().unwrap();
        assert_eq!(
            calls[0].2["query"]
                .as_str()
                .unwrap()
                .contains("repositoryContributions(first"),
            own
        );
    }
}

#[tokio::test]
async fn saturated_repository_ranges_are_split_and_only_leaf_results_count() {
    let names = vec!["org/truncated"; 100];
    let api=mock(vec![ok(json!({"data":{"user":{"range_0":repo_range(&names,false)}}})),
        ok(json!({"data":{"user":{"range_0":repo_range(&["org/left"],false),"range_1":repo_range(&["org/right"],false)}}}))]).await;
    let counts = fetch_all_time_stats(
        &api.state,
        "hiro",
        &[2023],
        &["all_time_contribs".into()],
        false,
    )
    .await
    .unwrap();
    assert_eq!(counts["all_time_contribs"], 2);
    let calls = api.data.calls.lock().unwrap();
    let query = calls[1].2["query"].as_str().unwrap();
    assert!(query.contains("2023-07-01T23:59:59Z"));
    assert!(query.contains("2023-07-02T00:00:00Z"));
}

#[tokio::test]
async fn search_uses_encoded_repo_owner_scopes_and_excludes_authors_from_comments() {
    let api = mock(vec![
        ok(json!({"total_count":12,"incomplete_results":false})),
        ok(json!({"total_count":7,"incomplete_results":false})),
    ])
    .await;
    let p = params(&[
        ("username", "hiro"),
        ("repo", "local,org/project"),
        ("owner", "other"),
        ("show", "prs_commented,issues_authored"),
    ]);
    let counts = fetch_search_stats(&api.state, "hiro", &p).await.unwrap();
    assert_eq!(counts["prs_commented"], 12);
    assert_eq!(counts["issues_authored"], 7);
    let calls = api.data.calls.lock().unwrap();
    assert_eq!(
        calls[0].1["q"],
        "repo:hiro/local repo:org/project owner:other commenter:hiro -author:hiro type:pr"
    );
    assert_eq!(calls[1].0, "/search/issues");
}

#[tokio::test]
async fn search_retries_partial_results_and_missing_all_time_data_is_an_error() {
    let api = mock(vec![
        ok(json!({"total_count":12,"incomplete_results":true}),);
        3
    ])
    .await;
    assert_eq!(
        search_count(&api.state, "issues", "author:hiro", &HashMap::new())
            .await
            .unwrap(),
        12
    );
    assert_eq!(api.data.calls.lock().unwrap().len(), 3);
    let api = mock(vec![ok(json!({"data":{"user":null}}))]).await;
    assert!(
        fetch_all_time_stats(
            &api.state,
            "hiro",
            &[2023],
            &["contributions".into()],
            false
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn pin_supports_other_owners_and_counts_the_requested_contributors_activity() {
    let api=mock(vec![ok(json!({"data":{"repository":{"name":"repo","nameWithOwner":"org/repo","isPrivate":false,"isArchived":false,"isTemplate":false,"stargazerCount":9}}})),ok(json!({"total_count":4}))]).await;
    let p = params(&[
        ("username", "hiro"),
        ("repo", "org/repo"),
        ("show", "prs_authored"),
    ]);
    let repo = fetch_extended_repo(&api.state, "hiro", &p).await.unwrap();
    assert_eq!(repo.extended_stats["prs_authored"], 4);
    let calls = api.data.calls.lock().unwrap();
    assert_eq!(calls[0].2["variables"]["login"], "org");
    assert_eq!(calls[1].1["q"], "repo:org/repo author:hiro type:pr");
}

#[tokio::test]
async fn language_scores_use_combined_bytes_and_repository_count_with_roles() {
    let api=mock(vec![ok(json!({"data":{"user":{"repositories":{"nodes":[
        {"name":"a","languages":{"edges":[{"size":100,"node":{"name":"Rust","color":"#dea584"}}]}},
        {"name":"b","languages":{"edges":[{"size":200,"node":{"name":"Rust","color":"#dea584"}},{"size":400,"node":{"name":"Python","color":"#3572A5"}}]}}
    ]}}}}))]).await;
    let langs = fetch_top_languages(
        &api.state,
        "hiro",
        vec![],
        1.0,
        1.0,
        &params(&[("role", "organization_member")]),
    )
    .await
    .unwrap();
    assert_eq!(langs[0].name, "Rust");
    assert_eq!(langs[0].size, 600.0);
    assert_eq!(
        api.data.calls.lock().unwrap()[0].2["variables"]["ownerAffiliations"],
        json!(["ORGANIZATION_MEMBER"])
    );
}

#[test]
fn adaptive_palettes_have_media_styles_and_separate_validated_gradients() {
    let p = params(&[
        ("theme_light", "light_github"),
        ("theme_dark", "dark_github"),
        ("bg_color_light", "45,ffffff,eeeeee"),
        ("bg_color_dark", "90,000000,222222"),
        ("prog_bar_bg_color_dark", "0000"),
    ]);
    let options = render_options_from_query(&p);
    let c = get_card_colors(&options, "default");
    assert!(c.theme_css.contains("prefers-color-scheme: dark"));
    assert!(c.theme_css.contains("#0969da"));
    assert!(c.theme_css.contains("#4493f8"));
    assert!(c.theme_defs.contains("gradient-light"));
    assert!(c.theme_defs.contains("gradient-dark"));
    let bad = render_options_from_query(&params(&[
        (
            "bg_color_dark",
            "0);}</style><script>alert(1)</script>,000,fff",
        ),
        ("text_color_light", "url(evil)"),
    ]));
    let c = get_card_colors(&bad, "default");
    assert!(!c.theme_css.contains("script"));
    assert!(!c.theme_defs.contains("script"));
    assert!(!c.theme_css.contains("evil"));
}

#[test]
fn hide_values_works_for_every_language_layout_without_removing_the_chart() {
    let langs = vec![
        LangData {
            name: "Rust".into(),
            color: "#dea584".into(),
            size: 75.0,
        },
        LangData {
            name: "Python".into(),
            color: "#3572a5".into(),
            size: 25.0,
        },
    ];
    for layout in ["normal", "compact", "donut", "donut-vertical", "pie"] {
        let p = params(&[
            ("layout", layout),
            ("hide_values", "true"),
            ("prog_bar_bg_color", "0000"),
        ]);
        let svg = render_top_languages_card(&langs, &p, &render_options_from_query(&p));
        assert!(svg.contains("Rust"));
        assert!(!svg.contains("75.00%"), "{layout}");
        assert!(svg.contains(if layout == "pie" {
            "lang-pie"
        } else if layout.starts_with("donut") {
            "lang-donut"
        } else {
            "lang-progress"
        }));
    }
    let svg = render_top_languages_card(
        &langs,
        &params(&[("prog_bar_bg_color", "0000")]),
        &render_options_from_query(&params(&[("prog_bar_bg_color", "0000")])),
    );
    assert!(svg.contains("#0000"));
    assert!(svg.contains("75.00%"));
}

#[test]
fn pixel_wrapping_handles_wide_letters_cjk_and_long_words_with_ellipsis() {
    assert!(
        wrap_description(&"W".repeat(50), 100.0, 10).len()
            > wrap_description(&"i".repeat(50), 100.0, 10).len()
    );
    let lines = wrap_description(&"日本語の説明文".repeat(10), 100.0, 3);
    assert_eq!(lines.len(), 3);
    assert!(lines[2].ends_with('…'));
    assert!(lines.iter().all(|line| measure_text(line, 13.0) <= 100.0));
}

#[test]
fn repository_width_and_extra_stats_and_animation_option_render_together() {
    let repo = RepoCardData {
        name: "repo".into(),
        name_with_owner: "org/repo".into(),
        description: Some("A useful repository".into()),
        primary_language_name: None,
        primary_language_color: None,
        is_archived: true,
        is_template: false,
        star_count: 5,
        fork_count: 2,
        extended_stats: BTreeMap::from([("prs_authored".into(), 3)]),
    };
    let p = params(&[
        ("card_width", "600"),
        ("disable_animations", "true"),
        ("theme_light", "light_github_repocard"),
        ("theme_dark", "dark_github_repocard"),
    ]);
    let svg = render_repo_card(&repo, &p, &render_options_from_query(&p));
    assert!(svg.contains("width=\"600\""));
    assert!(svg.contains("PRs Authored: 3"));
    assert!(svg.contains("translate(520,-18)"));
    assert!(svg.contains("animation-duration: 0s"));
    assert!(svg.contains("prefers-color-scheme: dark"));
}

#[test]
fn invalid_search_inputs_are_rejected_before_requests() {
    for p in [
        params(&[("username", "hiro OR author:other")]),
        params(&[("repo", "x\nowner:other")]),
        params(&[("commits_year", "NaN")]),
    ] {
        assert!(validate_search_params(&p).is_err());
    }
}

#[tokio::test]
async fn stats_handler_wires_new_counts_into_svg_and_response_cache() {
    let mut first = user(stars(5, None));
    first["data"]["user"]["contributionsCollection"] = json!({"contributionYears":[2023]});
    let api = mock(vec![
        ok(first),
        ok(json!({"total_count":11})),
        ok(json!({"data":{"user":{"year_0":{"contributionCalendar":{"totalContributions":123}}}}})),
        ok(json!({"data":{"user":{"range_0":repo_range(&["org/a","org/b"],false)}}})),
    ])
    .await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = api::router().with_state(api.state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let url = format!(
        "http://{addr}/api?username=hiro&show=contributions,all_time_contribs,prs_authored&disable_animations=true"
    );
    for _ in 0..2 {
        let response = Client::new().get(&url).send().await.unwrap();
        assert_eq!(response.headers()[CONTENT_TYPE], "image/svg+xml");
        let svg = response.text().await.unwrap();
        assert!(svg.contains("data-testid=\"contributions\">123</text>"));
        assert!(svg.contains("data-testid=\"all_time_contribs\">2</text>"));
        assert!(svg.contains("data-testid=\"prs_authored\">11</text>"));
    }
    assert_eq!(api.data.calls.lock().unwrap().len(), 4);
    task.abort();
}

#[test]
fn wakatime_hide_progress_preserves_language_labels_in_both_layouts() {
    let data = WakatimeData {
        range: None,
        is_coding_activity_visible: None,
        is_other_usage_visible: None,
        languages: vec![WakatimeLanguage {
            name: "Rust".into(),
            text: "1 hr".into(),
            percent: 100.0,
            _hours: None,
            _minutes: None,
        }],
    };
    for layout in ["normal", "compact"] {
        let p = params(&[("layout", layout), ("hide_progress", "true")]);
        let svg = render_wakatime_card(&data, &p, &RenderOptions::default());
        assert!(svg.contains("Rust"));
        assert!(!svg.contains("data-testid=\"lang-progress\""));
    }
}
