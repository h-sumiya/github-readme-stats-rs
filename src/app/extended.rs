//! API features adapted from stats-organization/github-stats-extended (MIT).
//! See THIRD_PARTY_NOTICES.md for the source revision and license.
use super::*;
use chrono::{Datelike, TimeZone};
use std::collections::VecDeque;

pub(super) const EXTENDED_STAT_LABELS: &[(&str, &str, &str)] = &[
    ("contributions", "Total Contributions", ICON_CONTRIBS),
    (
        "all_time_contribs",
        "Contributed to (all time)",
        ICON_CONTRIBS,
    ),
    ("prs_authored", "PRs Authored", ICON_PRS),
    ("prs_commented", "PRs Commented", ICON_PRS),
    ("prs_reviewed", "PRs Reviewed", ICON_REVIEWS),
    ("issues_authored", "Issues Authored", ICON_ISSUES),
    ("issues_commented", "Issues Commented", ICON_ISSUES),
];

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ContributionYears {
    #[serde(rename = "contributionYears")]
    pub years: Vec<i32>,
}

pub(super) fn parse_star_page_limit(value: Option<&str>) -> usize {
    match value {
        Some(v) if v.eq_ignore_ascii_case("true") => usize::MAX,
        Some(v) => v.parse::<usize>().ok().filter(|&n| n > 0).unwrap_or(1),
        None => 1,
    }
}

pub(super) fn owner_affiliations(
    params: &HashMap<String, String>,
) -> Result<Vec<String>, AppError> {
    let roles = parse_csv_param(params, "role");
    let roles: Vec<_> = if roles.is_empty() {
        vec!["OWNER".to_string()]
    } else {
        roles.into_iter().map(|v| v.to_ascii_uppercase()).collect()
    };
    if roles
        .iter()
        .any(|v| !["OWNER", "COLLABORATOR", "ORGANIZATION_MEMBER"].contains(&v.as_str()))
    {
        return Err(AppError::Message("Invalid repository role".to_string()));
    }
    Ok(roles)
}

pub(super) fn safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-/.,".contains(c))
}

pub(super) fn validate_search_params(params: &HashMap<String, String>) -> Result<(), AppError> {
    for name in ["username", "repo", "owner"] {
        if let Some(value) = q(params, name)
            && !safe_identifier(value)
        {
            return Err(AppError::Message(format!(
                "Invalid input for parameter \"{name}\""
            )));
        }
    }
    if let Some(year) = q(params, "commits_year")
        && !year
            .parse::<i32>()
            .is_ok_and(|v| (1970..=Utc::now().year()).contains(&v))
    {
        return Err(AppError::Message("Invalid commits_year".to_string()));
    }
    Ok(())
}

// Retry network failures on the same token; rotate credentials only for authentication/rate limits.
pub(super) async fn github_json(
    state: &AppState,
    path: &str,
    query: Option<&str>,
    body: Option<Value>,
) -> Result<Value, AppError> {
    if state.config.tokens.is_empty() {
        return Err(AppError::NoTokens);
    }
    let total = state.config.tokens.len();
    let start = state.next_token.fetch_add(1, AtomicOrdering::Relaxed) % total;
    for offset in 0..total {
        let token = &state.config.tokens[(start + offset) % total];
        let url = format!("{}{path}", state.config.api_base_url);
        let mut request = if let Some(ref body) = body {
            state.github_client.post(&url).json(body)
        } else {
            state.github_client.get(&url)
        };
        request = request
            .bearer_auth(&token.token)
            .header("Accept", "application/vnd.github+json");
        if let Some(query) = query {
            request = request.query(&[("q", query), ("per_page", "1")]);
        }
        let mut attempt = 0;
        let (status, text) = loop {
            let result = async {
                let response = request
                    .try_clone()
                    .expect("replayable JSON request")
                    .send()
                    .await?;
                let status = response.status();
                let text = response.text().await?;
                Ok::<_, reqwest::Error>((status, text))
            }
            .await;
            match result {
                Ok(value) => break value,
                Err(_) if attempt < 3 => {
                    tokio::time::sleep(Duration::from_millis([100, 1000, 3000][attempt])).await;
                    attempt += 1;
                }
                Err(_) => {
                    return Err(AppError::Message(
                        "Could not connect to GitHub after retries".to_string(),
                    ));
                }
            }
        };
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::TOO_MANY_REQUESTS {
            continue;
        }
        let raw: Value = serde_json::from_str(&text).map_err(|_| {
            AppError::Message(format!("Invalid JSON response from GitHub (HTTP {status})"))
        })?;
        let message = raw
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if message.contains("bad credentials")
            || message.contains("account was suspended")
            || (status == StatusCode::FORBIDDEN && message.contains("rate limit"))
        {
            continue;
        }
        if !status.is_success() {
            return Err(AppError::Message(format!("GitHub returned HTTP {status}")));
        }
        if let Some(errors) = raw
            .get("errors")
            .and_then(Value::as_array)
            .filter(|v| !v.is_empty())
        {
            if errors.iter().any(|e| {
                e["type"] == "RATE_LIMITED"
                    || e["message"]
                        .as_str()
                        .is_some_and(|v| v.to_ascii_lowercase().contains("rate limit"))
            }) {
                continue;
            }
            if errors.iter().any(|e| e["type"] == "NOT_FOUND") {
                return Err(AppError::UserNotFound);
            }
            let message = errors[0]["message"]
                .as_str()
                .unwrap_or("GitHub GraphQL error");
            return Err(AppError::Message(wrap_line(message, 90)));
        }
        return Ok(raw);
    }
    Err(AppError::MaxRetry)
}

fn search_filter(username: &str, params: &HashMap<String, String>) -> String {
    let mut filters = Vec::new();
    for repo in parse_csv_param(params, "repo") {
        let repo = if repo.contains('/') {
            repo
        } else {
            format!("{username}/{repo}")
        };
        filters.push(format!("repo:{repo}"));
    }
    filters.extend(
        parse_csv_param(params, "owner")
            .into_iter()
            .map(|owner| format!("owner:{owner}")),
    );
    filters.join(" ")
}

pub(super) async fn search_count(
    state: &AppState,
    kind: &str,
    filter: &str,
    params: &HashMap<String, String>,
) -> Result<i64, AppError> {
    let username = q(params, "username").unwrap_or_default();
    let query = format!("{} {filter}", search_filter(username, params));
    for attempt in 0..3 {
        let result =
            github_json(state, &format!("/search/{kind}"), Some(query.trim()), None).await?;
        // Search may initially return a partial total while its index catches up.
        if result.get("incomplete_results").and_then(Value::as_bool) == Some(true) {
            if attempt < 2 {
                tokio::time::sleep(Duration::from_millis([100, 1000][attempt])).await;
                continue;
            }
            tracing::warn!("GitHub search returned an incomplete result after retries");
        }
        return result
            .get("total_count")
            .and_then(Value::as_i64)
            .ok_or_else(|| AppError::Message("GitHub search did not return a count".to_string()));
    }
    unreachable!("bounded search retries always return")
}

pub(super) async fn fetch_search_stats(
    state: &AppState,
    username: &str,
    params: &HashMap<String, String>,
) -> Result<BTreeMap<String, i64>, AppError> {
    validate_search_params(params)?;
    let show = parse_csv_param(params, "show");
    let mut stats = BTreeMap::new();
    for (id, filter) in [
        ("prs_authored", format!("author:{username} type:pr")),
        (
            "prs_commented",
            format!("commenter:{username} -author:{username} type:pr"),
        ),
        (
            "prs_reviewed",
            format!("reviewed-by:{username} -author:{username} type:pr"),
        ),
        ("issues_authored", format!("author:{username} type:issue")),
        (
            "issues_commented",
            format!("commenter:{username} -author:{username} type:issue"),
        ),
    ] {
        if show.iter().any(|s| s == id) {
            stats.insert(
                id.to_string(),
                search_count(state, "issues", &filter, params).await?,
            );
        }
    }
    Ok(stats)
}

pub(super) async fn fetch_extended_repo(
    state: &AppState,
    username: &str,
    params: &HashMap<String, String>,
) -> Result<RepoCardData, AppError> {
    validate_search_params(params)?;
    let repo = q(params, "repo").unwrap_or_default();
    let (owner, name) = repo.split_once('/').unwrap_or((username, repo));
    let contributor = if username.is_empty() { owner } else { username };
    let mut data = fetch_repo(state, owner, name).await?;
    let mut scoped = params.clone();
    scoped.insert("username".to_string(), contributor.to_string());
    scoped.insert("repo".to_string(), format!("{owner}/{name}"));
    scoped.remove("owner");
    data.extended_stats = fetch_search_stats(state, contributor, &scoped).await?;
    Ok(data)
}

type DateRange = (DateTime<Utc>, DateTime<Utc>);
fn year_range(year: i32) -> Result<DateRange, AppError> {
    if !(1970..=Utc::now().year()).contains(&year) {
        return Err(AppError::Message(
            "Invalid contribution year from GitHub".to_string(),
        ));
    }
    Ok((
        Utc.with_ymd_and_hms(year, 1, 1, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(year, 12, 31, 23, 59, 59).unwrap(),
    ))
}
fn timestamp(date: DateTime<Utc>) -> String {
    date.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub(super) async fn fetch_all_time_stats(
    state: &AppState,
    username: &str,
    years: &[i32],
    show: &[String],
    include_own: bool,
) -> Result<BTreeMap<String, i64>, AppError> {
    let mut result = BTreeMap::new();
    let ranges: Vec<_> = years
        .iter()
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .map(year_range)
        .collect::<Result<_, _>>()?;
    if show.iter().any(|v| v == "contributions") {
        let mut total = 0;
        for chunk in ranges.chunks(10) {
            let fields = chunk.iter().enumerate().map(|(i, &(from, to))| format!(
                "year_{i}: contributionsCollection(from: \"{}\", to: \"{}\") {{ contributionCalendar {{ totalContributions }} }}", timestamp(from), timestamp(to)
            )).collect::<Vec<_>>().join("\n");
            let data = github_json(state, "/graphql", None, Some(json!({
                "query": format!("query($login: String!) {{ user(login: $login) {{ {fields} }} }}"),
                "variables": {"login": username}
            }))).await?;
            for i in 0..chunk.len() {
                total += data["data"]["user"][format!("year_{i}")]["contributionCalendar"]["totalContributions"].as_i64()
                    .ok_or_else(|| AppError::Message("Missing contribution calendar from GitHub".to_string()))?;
            }
        }
        result.insert("contributions".to_string(), total);
    }
    if show.iter().any(|v| v == "all_time_contribs") {
        result.insert(
            "all_time_contribs".to_string(),
            fetch_contributed_repositories(state, username, ranges, include_own).await?,
        );
    }
    Ok(result)
}

async fn fetch_contributed_repositories(
    state: &AppState,
    username: &str,
    ranges: Vec<DateRange>,
    include_own: bool,
) -> Result<i64, AppError> {
    let mut pending = VecDeque::from(ranges);
    let mut repositories = HashSet::new();
    let mut batch_size = 10;
    while !pending.is_empty() {
        let chunk: Vec<_> = pending.drain(..pending.len().min(batch_size)).collect();
        let fields = chunk.iter().enumerate().map(|(i, &(from, to))| format!(
            "range_{i}: contributionsCollection(from: \"{}\", to: \"{}\") {{ ...RangeRepos }}", timestamp(from), timestamp(to)
        )).collect::<Vec<_>>().join("\n");
        let own_field = if include_own {
            "repositoryContributions(first: 100) { nodes { repository { nameWithOwner } } }"
        } else {
            ""
        };
        let query = format!(
            "query($login: String!) {{ user(login: $login) {{ {fields} }} }} fragment RangeRepos on ContributionsCollection {{ commitContributionsByRepository(maxRepositories: 100) {{ repository {{ nameWithOwner }} }} issueContributionsByRepository(maxRepositories: 100) {{ repository {{ nameWithOwner }} }} pullRequestContributionsByRepository(maxRepositories: 100) {{ repository {{ nameWithOwner }} }} {own_field} }}"
        );
        let response = github_json(
            state,
            "/graphql",
            None,
            Some(json!({"query": query, "variables": {"login": username}})),
        )
        .await;
        let data = match response {
            Ok(data) => data,
            Err(AppError::Message(ref msg))
                if batch_size > 1
                    && (msg.contains("502")
                        || msg.contains("504")
                        || msg.to_ascii_lowercase().contains("resource")) =>
            {
                batch_size = (chunk.len() / 2).max(1);
                pending.extend(chunk);
                continue;
            }
            Err(err) => return Err(err),
        };
        for (i, (from, to)) in chunk.into_iter().enumerate() {
            let range = &data["data"]["user"][format!("range_{i}")];
            let mut entries = Vec::new();
            let mut saturated = false;
            for key in [
                "commitContributionsByRepository",
                "issueContributionsByRepository",
                "pullRequestContributionsByRepository",
            ] {
                let list = range[key].as_array().ok_or_else(|| {
                    AppError::Message("Missing repository contributions from GitHub".to_string())
                })?;
                saturated |= list.len() >= 100;
                entries.extend(list.iter());
            }
            if include_own {
                let list = range["repositoryContributions"]["nodes"]
                    .as_array()
                    .ok_or_else(|| {
                        AppError::Message(
                            "Missing owned repository contributions from GitHub".to_string(),
                        )
                    })?;
                saturated |= list.len() >= 100;
                entries.extend(list.iter());
            }
            let days = (to - from).num_seconds() / 86_400 + 1;
            if saturated && days > 1 {
                let middle = from + chrono::Duration::days(days / 2);
                pending.push_back((from, middle - chrono::Duration::seconds(1)));
                pending.push_back((middle, to));
                continue;
            }
            if saturated {
                return Err(AppError::Message(
                    "GitHub's daily repository limit prevents an exact all-time count".to_string(),
                ));
            }
            for entry in entries {
                let name = entry["repository"]["nameWithOwner"]
                    .as_str()
                    .ok_or_else(|| {
                        AppError::Message("Missing repository name from GitHub".to_string())
                    })?;
                if include_own
                    || !name
                        .split('/')
                        .next()
                        .unwrap_or_default()
                        .eq_ignore_ascii_case(username)
                {
                    repositories.insert(name.to_ascii_lowercase());
                }
            }
        }
    }
    Ok(repositories.len() as i64)
}

pub(super) fn has_palette(params: &HashMap<String, String>, suffix: &str) -> bool {
    [
        "theme",
        "title_color",
        "icon_color",
        "text_color",
        "ring_color",
        "bg_color",
        "border_color",
        "prog_bar_bg_color",
    ]
    .iter()
    .any(|key| params.contains_key(&format!("{key}{suffix}")))
}

pub(super) fn palette_options(
    params: &HashMap<String, String>,
    suffix: &str,
    base: &RenderOptions,
) -> RenderOptions {
    let get = |name: &str, fallback: &Option<String>| {
        q(params, &format!("{name}{suffix}"))
            .map(str::to_string)
            .or_else(|| fallback.clone())
    };
    RenderOptions {
        title_color: get("title_color", &base.title_color),
        icon_color: get("icon_color", &base.icon_color),
        text_color: get("text_color", &base.text_color),
        ring_color: get("ring_color", &base.ring_color),
        bg_color: get("bg_color", &base.bg_color),
        border_color: get("border_color", &base.border_color),
        prog_bar_bg_color: get("prog_bar_bg_color", &base.prog_bar_bg_color),
        theme: get("theme", &base.theme),
        light: None,
        dark: None,
    }
}

fn palette_background(bg: &BackgroundFill, id: &str) -> (String, String) {
    match bg {
        BackgroundFill::Solid(color) => (color.clone(), String::new()),
        BackgroundFill::Gradient { angle, colors } => {
            let stops = colors
                .iter()
                .enumerate()
                .map(|(i, color)| {
                    format!(
                        r#"<stop offset="{}%" stop-color="{color}"/>"#,
                        i * 100 / (colors.len() - 1)
                    )
                })
                .collect::<String>();
            (
                format!("url(#{id})"),
                format!(
                    r#"<defs><linearGradient id="{id}" gradientTransform="rotate({angle})">{stops}</linearGradient></defs>"#
                ),
            )
        }
    }
}

pub(super) fn get_card_colors(options: &RenderOptions, default_theme: &str) -> CardColors {
    if options.light.is_none() && options.dark.is_none() {
        return solid_card_colors(options, default_theme);
    }
    let light = solid_card_colors(options.light.as_deref().unwrap_or(options), default_theme);
    let dark = solid_card_colors(options.dark.as_deref().unwrap_or(options), default_theme);
    let (light_bg, light_defs) = palette_background(&light.bg_color, "gradient-light");
    let (dark_bg, dark_defs) = palette_background(&dark.bg_color, "gradient-dark");
    let variables = |palette: &CardColors, bg: &str| {
        format!(
            "--card-title:{};--card-icon:{};--card-text:{};--card-ring:{};--card-border:{};--card-bg:{bg};--card-progress:{};",
            palette.title_color,
            palette.icon_color,
            palette.text_color,
            palette.ring_color,
            palette.border_color,
            palette.prog_bar_bg_color
        )
    };
    CardColors {
        title_color: "var(--card-title)".into(),
        icon_color: "var(--card-icon)".into(),
        text_color: "var(--card-text)".into(),
        ring_color: "var(--card-ring)".into(),
        border_color: "var(--card-border)".into(),
        bg_color: BackgroundFill::Solid("var(--card-bg)".into()),
        prog_bar_bg_color: "var(--card-progress)".into(),
        theme_css: format!(
            ":root {{ {} }} @media (prefers-color-scheme: dark) {{ :root {{ {} }} }}",
            variables(&light, &light_bg),
            variables(&dark, &dark_bg)
        ),
        theme_defs: format!("{light_defs}{dark_defs}"),
    }
}

pub(super) fn wrap_description(text: &str, width: f64, max_lines: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if measure_text(&candidate, 13.0) <= width {
            line = candidate;
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        // Break long words, including CJK text, without splitting UTF-8 characters.
        for ch in word.chars() {
            if measure_text(&format!("{line}{ch}"), 13.0) > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            line.push(ch);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            while measure_text(&format!("{last}…"), 13.0) > width && !last.is_empty() {
                last.pop();
            }
            last.push('…');
        }
    }
    lines
}
