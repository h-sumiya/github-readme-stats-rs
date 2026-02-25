use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::f64::consts::PI;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, EXPIRES, PRAGMA};
use axum::http::{HeaderValue, Response, StatusCode};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use once_cell::sync::Lazy;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

mod api;

const DAY_SECONDS: u64 = 24 * 60 * 60;
const ERROR_CACHE_SECONDS: u64 = 10 * 60;

const BLACKLIST: &[&str] = &[
    "renovate-bot",
    "technote-space",
    "sw-yx",
    "YourUsername",
    "[YourUsername]",
];

const ICON_STAR: &str = r#"<path fill-rule="evenodd" d="M8 .25a.75.75 0 01.673.418l1.882 3.815 4.21.612a.75.75 0 01.416 1.279l-3.046 2.97.719 4.192a.75.75 0 01-1.088.791L8 12.347l-3.766 1.98a.75.75 0 01-1.088-.79l.72-4.194L.818 6.374a.75.75 0 01.416-1.28l4.21-.611L7.327.668A.75.75 0 018 .25zm0 2.445L6.615 5.5a.75.75 0 01-.564.41l-3.097.45 2.24 2.184a.75.75 0 01.216.664l-.528 3.084 2.769-1.456a.75.75 0 01.698 0l2.77 1.456-.53-3.084a.75.75 0 01.216-.664l2.24-2.183-3.096-.45a.75.75 0 01-.564-.41L8 2.694v.001z"/>"#;
const ICON_COMMITS: &str = r#"<path fill-rule="evenodd" d="M1.643 3.143L.427 1.927A.25.25 0 000 2.104V5.75c0 .138.112.25.25.25h3.646a.25.25 0 00.177-.427L2.715 4.215a6.5 6.5 0 11-1.18 4.458.75.75 0 10-1.493.154 8.001 8.001 0 101.6-5.684zM7.75 4a.75.75 0 01.75.75v2.992l2.028.812a.75.75 0 01-.557 1.392l-2.5-1A.75.75 0 017 8.25v-3.5A.75.75 0 017.75 4z"/>"#;
const ICON_PRS: &str = r#"<path fill-rule="evenodd" d="M7.177 3.073L9.573.677A.25.25 0 0110 .854v4.792a.25.25 0 01-.427.177L7.177 3.427a.25.25 0 010-.354zM3.75 2.5a.75.75 0 100 1.5.75.75 0 000-1.5zm-2.25.75a2.25 2.25 0 113 2.122v5.256a2.251 2.251 0 11-1.5 0V5.372A2.25 2.25 0 011.5 3.25zM11 2.5h-1V4h1a1 1 0 011 1v5.628a2.251 2.251 0 101.5 0V5A2.5 2.5 0 0011 2.5zm1 10.25a.75.75 0 111.5 0 .75.75 0 01-1.5 0zM3.75 12a.75.75 0 100 1.5.75.75 0 000-1.5z"/>"#;
const ICON_ISSUES: &str = r#"<path fill-rule="evenodd" d="M8 1.5a6.5 6.5 0 100 13 6.5 6.5 0 000-13zM0 8a8 8 0 1116 0A8 8 0 010 8zm9 3a1 1 0 11-2 0 1 1 0 012 0zm-.25-6.25a.75.75 0 00-1.5 0v3.5a.75.75 0 001.5 0v-3.5z"/>"#;
const ICON_CONTRIBS: &str = r#"<path fill-rule="evenodd" d="M2 2.5A2.5 2.5 0 014.5 0h8.75a.75.75 0 01.75.75v12.5a.75.75 0 01-.75.75h-2.5a.75.75 0 110-1.5h1.75v-2h-8a1 1 0 00-.714 1.7.75.75 0 01-1.072 1.05A2.495 2.495 0 012 11.5v-9zm10.5-1V9h-8c-.356 0-.694.074-1 .208V2.5a1 1 0 011-1h8zM5 12.25v3.25a.25.25 0 00.4.2l1.45-1.087a.25.25 0 01.3 0L8.6 15.7a.25.25 0 00.4-.2v-3.25a.25.25 0 00-.25-.25h-3.5a.25.25 0 00-.25.25z"/>"#;
const ICON_FORK: &str = r#"<path fill-rule="evenodd" d="M5 3.25a.75.75 0 11-1.5 0 .75.75 0 011.5 0zm0 2.122a2.25 2.25 0 10-1.5 0v.878A2.25 2.25 0 005.75 8.5h1.5v2.128a2.251 2.251 0 101.5 0V8.5h1.5a2.25 2.25 0 002.25-2.25v-.878a2.25 2.25 0 10-1.5 0v.878a.75.75 0 01-.75.75h-4.5A.75.75 0 015 6.25v-.878zm3.75 7.378a.75.75 0 11-1.5 0 .75.75 0 011.5 0zm3-8.75a.75.75 0 100-1.5.75.75 0 000 1.5z"></path>"#;
const ICON_GIST: &str = r#"<path fill-rule="evenodd" d="M0 1.75C0 .784.784 0 1.75 0h12.5C15.216 0 16 .784 16 1.75v12.5A1.75 1.75 0 0 1 14.25 16H1.75A1.75 1.75 0 0 1 0 14.25Zm1.75-.25a.25.25 0 0 0-.25.25v12.5c0 .138.112.25.25.25h12.5a.25.25 0 0 0 .25-.25V1.75a.25.25 0 0 0-.25-.25Zm7.47 3.97a.75.75 0 0 1 1.06 0l2 2a.75.75 0 0 1 0 1.06l-2 2a.749.749 0 0 1-1.275-.326.749.749 0 0 1 .215-.734L10.69 8 9.22 6.53a.75.75 0 0 1 0-1.06ZM6.78 6.53 5.31 8l1.47 1.47a.749.749 0 0 1-.326 1.275.749.749 0 0 1-.734-.215l-2-2a.75.75 0 0 1 0-1.06l2-2a.751.751 0 0 1 1.042.018.751.751 0 0 1 .018 1.042Z" />"#;
const ICON_REVIEWS: &str = r#"<path fill-rule="evenodd" d="M8 2c1.981 0 3.671.992 4.933 2.078 1.27 1.091 2.187 2.345 2.637 3.023a1.62 1.62 0 0 1 0 1.798c-.45.678-1.367 1.932-2.637 3.023C11.67 13.008 9.981 14 8 14c-1.981 0-3.671-.992-4.933-2.078C1.797 10.83.88 9.576.43 8.898a1.62 1.62 0 0 1 0-1.798c.45-.677 1.367-1.931 2.637-3.022C4.33 2.992 6.019 2 8 2ZM1.679 7.932a.12.12 0 0 0 0 .136c.411.622 1.241 1.75 2.366 2.717C5.176 11.758 6.527 12.5 8 12.5c1.473 0 2.825-.742 3.955-1.715 1.124-.967 1.954-2.096 2.366-2.717a.12.12 0 0 0 0-.136c-.412-.621-1.242-1.75-2.366-2.717C10.824 4.242 9.473 3.5 8 3.5c-1.473 0-2.825.742-3.955 1.715-1.124.967-1.954 2.096-2.366 2.717ZM8 10a2 2 0 1 1-.001-3.999A2 2 0 0 1 8 10Z"/>"#;
const ICON_DISCUSSIONS_STARTED: &str = r#"<path fill-rule="evenodd" d="M1.75 1h8.5c.966 0 1.75.784 1.75 1.75v5.5A1.75 1.75 0 0 1 10.25 10H7.061l-2.574 2.573A1.458 1.458 0 0 1 2 11.543V10h-.25A1.75 1.75 0 0 1 0 8.25v-5.5C0 1.784.784 1 1.75 1ZM1.5 2.75v5.5c0 .138.112.25.25.25h1a.75.75 0 0 1 .75.75v2.19l2.72-2.72a.749.749 0 0 1 .53-.22h3.5a.25.25 0 0 0 .25-.25v-5.5a.25.25 0 0 0-.25-.25h-8.5a.25.25 0 0 0-.25.25Zm13 2a.25.25 0 0 0-.25-.25h-.5a.75.75 0 0 1 0-1.5h.5c.966 0 1.75.784 1.75 1.75v5.5A1.75 1.75 0 0 1 14.25 12H14v1.543a1.458 1.458 0 0 1-2.487 1.03L9.22 12.28a.749.749 0 0 1 .326-1.275.749.749 0 0 1 .734.215l2.22 2.22v-2.19a.75.75 0 0 1 .75-.75h1a.25.25 0 0 0 .25-.25Z" />"#;
const ICON_DISCUSSIONS_ANSWERED: &str = r#"<path fill-rule="evenodd" d="M13.78 4.22a.75.75 0 0 1 0 1.06l-7.25 7.25a.75.75 0 0 1-1.06 0L2.22 9.28a.751.751 0 0 1 .018-1.042.751.751 0 0 1 1.042-.018L6 10.94l6.72-6.72a.75.75 0 0 1 1.06 0Z" />"#;
const ICON_PRS_MERGED: &str = r#"<path fill-rule="evenodd" d="M5.45 5.154A4.25 4.25 0 0 0 9.25 7.5h1.378a2.251 2.251 0 1 1 0 1.5H9.25A5.734 5.734 0 0 1 5 7.123v3.505a2.25 2.25 0 1 1-1.5 0V5.372a2.25 2.25 0 1 1 1.95-.218ZM4.25 13.5a.75.75 0 1 0 0-1.5.75.75 0 0 0 0 1.5Zm8.5-4.5a.75.75 0 1 0 0-1.5.75.75 0 0 0 0 1.5ZM5 3.25a.75.75 0 1 0 0 .005V3.25Z" />"#;
const ICON_PRS_MERGED_PERCENT: &str = r#"<path fill-rule="evenodd" d="M13.442 2.558a.625.625 0 0 1 0 .884l-10 10a.625.625 0 1 1-.884-.884l10-10a.625.625 0 0 1 .884 0zM4.5 6a1.5 1.5 0 1 1 0-3 1.5 1.5 0 0 1 0 3zm0 1a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5zm7 6a1.5 1.5 0 1 1 0-3 1.5 1.5 0 0 1 0 3zm0 1a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5z" />"#;

const STATS_QUERY: &str = r#"
query userInfo($login: String!, $after: String, $includeMergedPullRequests: Boolean!, $includeDiscussions: Boolean!, $includeDiscussionsAnswers: Boolean!, $startTime: DateTime = null) {
  user(login: $login) {
    name
    login
    commits: contributionsCollection (from: $startTime) {
      totalCommitContributions
    }
    reviews: contributionsCollection {
      totalPullRequestReviewContributions
    }
    repositoriesContributedTo(first: 1, contributionTypes: [COMMIT, ISSUE, PULL_REQUEST, REPOSITORY]) {
      totalCount
    }
    pullRequests(first: 1) {
      totalCount
    }
    mergedPullRequests: pullRequests(states: MERGED) @include(if: $includeMergedPullRequests) {
      totalCount
    }
    openIssues: issues(states: OPEN) {
      totalCount
    }
    closedIssues: issues(states: CLOSED) {
      totalCount
    }
    followers {
      totalCount
    }
    repositoryDiscussions @include(if: $includeDiscussions) {
      totalCount
    }
    repositoryDiscussionComments(onlyAnswers: true) @include(if: $includeDiscussionsAnswers) {
      totalCount
    }
    repositories(first: 100, ownerAffiliations: OWNER, orderBy: {direction: DESC, field: STARGAZERS}, after: $after) {
      totalCount
      nodes {
        name
        stargazers {
          totalCount
        }
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
  }
}
"#;

const STATS_REPOS_QUERY: &str = r#"
query userInfo($login: String!, $after: String) {
  user(login: $login) {
    repositories(first: 100, ownerAffiliations: OWNER, orderBy: {direction: DESC, field: STARGAZERS}, after: $after) {
      totalCount
      nodes {
        name
        stargazers {
          totalCount
        }
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
  }
}
"#;

const TOP_LANGS_QUERY: &str = r#"
query userInfo($login: String!) {
  user(login: $login) {
    repositories(ownerAffiliations: OWNER, isFork: false, first: 100) {
      nodes {
        name
        languages(first: 10, orderBy: {field: SIZE, direction: DESC}) {
          edges {
            size
            node {
              color
              name
            }
          }
        }
      }
    }
  }
}
"#;

const REPO_QUERY: &str = r#"
fragment RepoInfo on Repository {
  name
  nameWithOwner
  isPrivate
  isArchived
  isTemplate
  stargazers {
    totalCount
  }
  description
  primaryLanguage {
    color
    id
    name
  }
  forkCount
}
query getRepo($login: String!, $repo: String!) {
  user(login: $login) {
    repository(name: $repo) {
      ...RepoInfo
    }
  }
  organization(login: $login) {
    repository(name: $repo) {
      ...RepoInfo
    }
  }
}
"#;

const GIST_QUERY: &str = r#"
query gistInfo($gistName: String!) {
  viewer {
    gist(name: $gistName) {
      description
      owner {
        login
      }
      stargazerCount
      forks {
        totalCount
      }
      files {
        name
        language {
          name
        }
        size
      }
    }
  }
}
"#;

const RATE_LIMIT_QUERY: &str = r#"
query {
  rateLimit {
    remaining
    resetAt
  }
}
"#;

static THEMES: Lazy<HashMap<String, ThemeDefinition>> = Lazy::new(|| {
    serde_json::from_str(include_str!("../assets/themes.json")).expect("valid themes.json")
});

static LANGUAGE_COLORS: Lazy<HashMap<String, String>> = Lazy::new(|| {
    serde_json::from_str(include_str!("../assets/language-colors.json"))
        .expect("valid language-colors.json")
});

#[derive(Debug, Clone, Deserialize)]
struct ThemeDefinition {
    #[serde(default)]
    title_color: String,
    #[serde(default)]
    icon_color: String,
    #[serde(default)]
    text_color: String,
    #[serde(default)]
    bg_color: String,
    border_color: Option<String>,
    ring_color: Option<String>,
}

#[derive(Clone)]
struct TokenEntry {
    name: String,
    token: String,
}

#[derive(Clone)]
struct AppConfig {
    host: String,
    port: u16,
    node_env: String,
    cache_seconds_override: Option<u64>,
    fetch_multi_page_stars: bool,
    tokens: Vec<TokenEntry>,
    exclude_repos: HashSet<String>,
    whitelist: Option<HashSet<String>>,
    gist_whitelist: Option<HashSet<String>>,
}

impl AppConfig {
    fn from_env() -> Self {
        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8080);
        let node_env = env::var("NODE_ENV").unwrap_or_else(|_| "production".to_string());
        let cache_seconds_override = env::var("CACHE_SECONDS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok());
        let fetch_multi_page_stars = env::var("FETCH_MULTI_PAGE_STARS")
            .ok()
            .map(|v| v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let mut pat_vars: Vec<(usize, TokenEntry)> = env::vars()
            .filter_map(|(k, v)| {
                let idx = k.strip_prefix("PAT_")?.parse::<usize>().ok()?;
                Some((idx, TokenEntry { name: k, token: v }))
            })
            .collect();

        pat_vars.sort_by_key(|(idx, _)| *idx);

        let mut tokens: Vec<TokenEntry> = pat_vars.into_iter().map(|(_, t)| t).collect();

        if tokens.is_empty() {
            if let Ok(token) = env::var("GITHUB_TOKEN") {
                tokens.push(TokenEntry {
                    name: "GITHUB_TOKEN".to_string(),
                    token,
                });
            }
        }

        let exclude_repos = split_csv(&env::var("EXCLUDE_REPO").unwrap_or_default())
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<HashSet<_>>();

        let whitelist = env::var("WHITELIST").ok().map(|v| {
            split_csv(&v)
                .into_iter()
                .filter(|x| !x.is_empty())
                .collect()
        });

        let gist_whitelist = env::var("GIST_WHITELIST").ok().map(|v| {
            split_csv(&v)
                .into_iter()
                .filter(|x| !x.is_empty())
                .collect()
        });

        Self {
            host,
            port,
            node_env,
            cache_seconds_override,
            fetch_multi_page_stars,
            tokens,
            exclude_repos,
            whitelist,
            gist_whitelist,
        }
    }
}

#[derive(Clone)]
struct AppState {
    config: Arc<AppConfig>,
    github_client: Client,
    http_client: Client,
    cache: Arc<DashMap<String, CachedResponse>>,
    next_token: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct CachedResponse {
    status: StatusCode,
    content_type: &'static str,
    cache_headers: CacheHeaders,
    body: Bytes,
    expires_at: Instant,
}

#[derive(Clone)]
enum CacheHeaders {
    NoCache,
    Control(String),
}

#[derive(Clone)]
struct ResponsePayload {
    status: StatusCode,
    content_type: &'static str,
    body: String,
    cache_headers: CacheHeaders,
    internal_ttl: Duration,
}

#[derive(Debug, Error)]
enum AppError {
    #[error("Missing params {0} make sure you pass the parameters in URL")]
    MissingParam(String),
    #[error("No GitHub API tokens found")]
    NoTokens,
    #[error("Downtime due to GitHub API rate limiting")]
    MaxRetry,
    #[error("Could not fetch user.")]
    UserNotFound,
    #[error("Could not resolve to a User with the login of '{0}'")]
    WakatimeUserNotFound(String),
    #[error("{0}")]
    Message(String),
}

impl AppError {
    fn secondary_message(&self) -> Option<&'static str> {
        match self {
            Self::NoTokens => Some("Please add PAT_1 with your GitHub API token"),
            Self::MaxRetry => {
                Some("You can deploy own instance or wait until public will be no longer limited")
            }
            Self::UserNotFound => Some("Make sure the provided username is not an organization"),
            Self::WakatimeUserNotFound(_) => Some("Make sure you have a public WakaTime profile"),
            _ => None,
        }
    }

    fn show_repo_link(&self) -> bool {
        !matches!(self, Self::MissingParam(_))
    }
}

#[derive(Default, Clone)]
struct RenderOptions {
    title_color: Option<String>,
    icon_color: Option<String>,
    text_color: Option<String>,
    ring_color: Option<String>,
    bg_color: Option<String>,
    border_color: Option<String>,
    theme: Option<String>,
}

#[derive(Clone)]
struct CardColors {
    title_color: String,
    icon_color: String,
    text_color: String,
    ring_color: String,
    border_color: String,
    bg_color: BackgroundFill,
}

#[derive(Clone)]
enum BackgroundFill {
    Solid(String),
    Gradient { angle: String, colors: Vec<String> },
}

#[derive(Deserialize)]
struct GraphQlEnvelope<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Debug, Clone, Deserialize)]
struct GraphQlError {
    #[serde(default)]
    message: String,
    #[serde(default, rename = "type")]
    kind: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CountNode {
    #[serde(default, rename = "totalCount")]
    total_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct PageInfo {
    #[serde(default, rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(default, rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct StarNode {
    name: String,
    stargazers: CountNode,
}

#[derive(Debug, Clone, Deserialize)]
struct RepositoriesConnection {
    #[serde(default, rename = "totalCount")]
    _total_count: i64,
    #[serde(default)]
    nodes: Vec<StarNode>,
    #[serde(default, rename = "pageInfo")]
    page_info: Option<PageInfo>,
}

#[derive(Debug, Clone, Deserialize)]
struct StatsContributions {
    #[serde(default, rename = "totalCommitContributions")]
    total_commit_contributions: i64,
    #[serde(default, rename = "totalPullRequestReviewContributions")]
    total_pull_request_review_contributions: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct StatsUser {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    login: String,
    commits: StatsContributions,
    reviews: StatsContributions,
    #[serde(rename = "repositoriesContributedTo")]
    repositories_contributed_to: CountNode,
    #[serde(rename = "pullRequests")]
    pull_requests: CountNode,
    #[serde(default, rename = "mergedPullRequests")]
    merged_pull_requests: Option<CountNode>,
    #[serde(rename = "openIssues")]
    open_issues: CountNode,
    #[serde(rename = "closedIssues")]
    closed_issues: CountNode,
    followers: CountNode,
    #[serde(default, rename = "repositoryDiscussions")]
    repository_discussions: Option<CountNode>,
    #[serde(default, rename = "repositoryDiscussionComments")]
    repository_discussion_comments: Option<CountNode>,
    repositories: RepositoriesConnection,
}

#[derive(Debug, Clone, Deserialize)]
struct StatsData {
    user: Option<StatsUser>,
}

#[derive(Debug, Clone, Deserialize)]
struct StatsReposOnlyUser {
    repositories: RepositoriesConnection,
}

#[derive(Debug, Clone, Deserialize)]
struct StatsReposOnlyData {
    user: Option<StatsReposOnlyUser>,
}

#[derive(Debug, Clone, Deserialize)]
struct TopLangEdgeNode {
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct TopLangEdge {
    #[serde(default)]
    size: f64,
    node: TopLangEdgeNode,
}

#[derive(Debug, Clone, Deserialize)]
struct TopLangEdgeConnection {
    #[serde(default)]
    edges: Vec<TopLangEdge>,
}

#[derive(Debug, Clone, Deserialize)]
struct TopLangRepo {
    name: String,
    languages: TopLangEdgeConnection,
}

#[derive(Debug, Clone, Deserialize)]
struct TopLangRepoConnection {
    #[serde(default)]
    nodes: Vec<TopLangRepo>,
}

#[derive(Debug, Clone, Deserialize)]
struct TopLangUser {
    repositories: TopLangRepoConnection,
}

#[derive(Debug, Clone, Deserialize)]
struct TopLangData {
    user: Option<TopLangUser>,
}

#[derive(Debug, Clone, Deserialize)]
struct RepoLanguage {
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RepoCardRepo {
    name: String,
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
    #[serde(rename = "isPrivate")]
    is_private: bool,
    #[serde(rename = "isArchived")]
    is_archived: bool,
    #[serde(rename = "isTemplate")]
    is_template: bool,
    stargazers: CountNode,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, rename = "primaryLanguage")]
    primary_language: Option<RepoLanguage>,
    #[serde(default, rename = "forkCount")]
    fork_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct RepoUserOrOrg {
    #[serde(default)]
    repository: Option<RepoCardRepo>,
}

#[derive(Debug, Clone, Deserialize)]
struct RepoData {
    #[serde(default)]
    user: Option<RepoUserOrOrg>,
    #[serde(default)]
    organization: Option<RepoUserOrOrg>,
}

#[derive(Debug, Clone, Deserialize)]
struct GistLanguage {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GistFile {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    language: Option<GistLanguage>,
    #[serde(default)]
    size: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct GistOwner {
    login: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GistForks {
    #[serde(default, rename = "totalCount")]
    total_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct GistNode {
    #[serde(default)]
    description: Option<String>,
    owner: GistOwner,
    #[serde(default, rename = "stargazerCount")]
    stargazer_count: i64,
    forks: GistForks,
    #[serde(default)]
    files: Vec<GistFile>,
}

#[derive(Debug, Clone, Deserialize)]
struct GistViewer {
    #[serde(default)]
    gist: Option<GistNode>,
}

#[derive(Debug, Clone, Deserialize)]
struct GistData {
    viewer: GistViewer,
}

#[derive(Debug, Clone)]
struct StatsCardData {
    name: String,
    total_prs: i64,
    total_prs_merged: i64,
    merged_prs_percentage: f64,
    total_reviews: i64,
    total_commits: i64,
    total_issues: i64,
    total_stars: i64,
    total_discussions_started: i64,
    total_discussions_answered: i64,
    contributed_to: i64,
    rank: Rank,
}

#[derive(Debug, Clone)]
struct Rank {
    level: String,
    percentile: f64,
}

#[derive(Debug, Clone)]
struct RepoCardData {
    name: String,
    name_with_owner: String,
    description: Option<String>,
    primary_language_name: Option<String>,
    primary_language_color: Option<String>,
    is_archived: bool,
    is_template: bool,
    star_count: i64,
    fork_count: i64,
}

#[derive(Debug, Clone)]
struct GistCardData {
    name: String,
    name_with_owner: String,
    description: Option<String>,
    language: Option<String>,
    stars_count: i64,
    forks_count: i64,
}

#[derive(Debug, Clone)]
struct LangData {
    name: String,
    color: String,
    size: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct WakatimeResponse {
    data: WakatimeData,
}

#[derive(Debug, Clone, Deserialize)]
struct WakatimeData {
    #[serde(default)]
    range: Option<String>,
    #[serde(default)]
    is_coding_activity_visible: Option<bool>,
    #[serde(default)]
    is_other_usage_visible: Option<bool>,
    #[serde(default)]
    languages: Vec<WakatimeLanguage>,
}

#[derive(Debug, Clone, Deserialize)]
struct WakatimeLanguage {
    name: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    percent: f64,
    #[serde(default, rename = "hours")]
    _hours: Option<i64>,
    #[serde(default, rename = "minutes")]
    _minutes: Option<i64>,
}

pub async fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = Arc::new(AppConfig::from_env());

    let github_client = Client::builder()
        .user_agent("github-readme-stats-rs/0.1.0")
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(32)
        .brotli(true)
        .gzip(true)
        .build()
        .expect("http client");

    let http_client = Client::builder()
        .user_agent("github-readme-stats-rs/0.1.0")
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(16)
        .brotli(true)
        .gzip(true)
        .build()
        .expect("http client");

    let state = AppState {
        config: config.clone(),
        github_client,
        http_client,
        cache: Arc::new(DashMap::new()),
        next_token: Arc::new(AtomicUsize::new(0)),
    };

    let cors_layer = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = api::router().with_state(state).layer(cors_layer);

    let bind_addr = format!("{}:{}", config.host, config.port);
    info!("listening on {bind_addr}");

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("bind address");
    axum::serve(listener, app).await.expect("serve");
}

async fn handle_svg_cached<F, Fut>(
    state: AppState,
    path: &'static str,
    params: HashMap<String, String>,
    build: F,
) -> Response<Body>
where
    F: FnOnce(AppState, HashMap<String, String>) -> Fut,
    Fut: std::future::Future<Output = ResponsePayload>,
{
    let key = cache_key(path, &params);
    if let Some(cached) = read_cache(&state, &key) {
        return response_from_cached(cached, true);
    }

    let payload = build(state.clone(), params).await;
    let cached = payload_to_cached(payload);
    if cached.expires_at > Instant::now() {
        state.cache.insert(key, cached.clone());
    }
    response_from_cached(cached, false)
}

async fn handle_json_cached<F, Fut>(
    state: AppState,
    path: &'static str,
    params: HashMap<String, String>,
    build: F,
) -> Response<Body>
where
    F: FnOnce(AppState, HashMap<String, String>) -> Fut,
    Fut: std::future::Future<Output = ResponsePayload>,
{
    let key = cache_key(path, &params);
    if let Some(cached) = read_cache(&state, &key) {
        return response_from_cached(cached, true);
    }

    let payload = build(state.clone(), params).await;
    let cached = payload_to_cached(payload);
    if cached.expires_at > Instant::now() {
        state.cache.insert(key, cached.clone());
    }
    response_from_cached(cached, false)
}

fn plain_text_response(text: &str) -> Response<Body> {
    let mut response = Response::new(Body::from(text.to_string()));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

fn payload_to_cached(payload: ResponsePayload) -> CachedResponse {
    let expires_at = if payload.internal_ttl.is_zero() {
        Instant::now()
    } else {
        Instant::now() + payload.internal_ttl
    };

    CachedResponse {
        status: payload.status,
        content_type: payload.content_type,
        cache_headers: payload.cache_headers,
        body: Bytes::from(payload.body),
        expires_at,
    }
}

fn read_cache(state: &AppState, key: &str) -> Option<CachedResponse> {
    if let Some(found) = state.cache.get(key) {
        let value = found.value().clone();
        if value.expires_at > Instant::now() {
            return Some(value);
        }
        drop(found);
        state.cache.remove(key);
    }
    None
}

fn response_from_cached(cached: CachedResponse, hit: bool) -> Response<Body> {
    let mut response = Response::new(Body::from(cached.body.clone()));
    *response.status_mut() = cached.status;

    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(cached.content_type));

    match cached.cache_headers {
        CacheHeaders::NoCache => {
            response.headers_mut().insert(
                CACHE_CONTROL,
                HeaderValue::from_static(
                    "no-cache, no-store, must-revalidate, max-age=0, s-maxage=0",
                ),
            );
            response
                .headers_mut()
                .insert(PRAGMA, HeaderValue::from_static("no-cache"));
            response
                .headers_mut()
                .insert(EXPIRES, HeaderValue::from_static("0"));
        }
        CacheHeaders::Control(value) => {
            if let Ok(v) = HeaderValue::from_str(&value) {
                response.headers_mut().insert(CACHE_CONTROL, v);
            }
        }
    }

    response.headers_mut().insert(
        "x-cache",
        HeaderValue::from_static(if hit { "HIT" } else { "MISS" }),
    );

    response
}

fn cache_key(path: &str, params: &HashMap<String, String>) -> String {
    let mut sorted = BTreeMap::new();
    for (k, v) in params {
        sorted.insert(k.clone(), v.clone());
    }
    let query = sorted
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    if query.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{query}")
    }
}

fn svg_success_payload(svg: String, cache_seconds: u64, config: &AppConfig) -> ResponsePayload {
    let cache_headers = if cache_seconds < 1 || config.node_env == "development" {
        CacheHeaders::NoCache
    } else {
        CacheHeaders::Control(format!(
            "max-age={cache_seconds}, s-maxage={cache_seconds}, stale-while-revalidate={DAY_SECONDS}"
        ))
    };
    ResponsePayload {
        status: StatusCode::OK,
        content_type: "image/svg+xml",
        body: svg,
        cache_headers,
        internal_ttl: if cache_seconds < 1 || config.node_env == "development" {
            Duration::from_secs(0)
        } else {
            Duration::from_secs(cache_seconds)
        },
    }
}

fn svg_error_payload(svg: String, config: &AppConfig) -> ResponsePayload {
    let cache_headers = if config.node_env == "development"
        || config
            .cache_seconds_override
            .map(|v| v < 1)
            .unwrap_or(false)
    {
        CacheHeaders::NoCache
    } else {
        CacheHeaders::Control(format!(
            "max-age={ERROR_CACHE_SECONDS}, s-maxage={ERROR_CACHE_SECONDS}, stale-while-revalidate={DAY_SECONDS}"
        ))
    };

    ResponsePayload {
        status: StatusCode::OK,
        content_type: "image/svg+xml",
        body: svg,
        cache_headers: cache_headers.clone(),
        internal_ttl: if matches!(cache_headers, CacheHeaders::NoCache) {
            Duration::from_secs(0)
        } else {
            Duration::from_secs(ERROR_CACHE_SECONDS)
        },
    }
}

fn resolve_cache_seconds(
    requested: Option<i64>,
    default_value: u64,
    min: u64,
    max: u64,
    config: &AppConfig,
) -> u64 {
    if let Some(override_value) = config.cache_seconds_override {
        return override_value;
    }

    let fallback = requested.unwrap_or(default_value as i64).max(0) as u64;
    fallback.clamp(min, max)
}

#[derive(Clone, Copy)]
enum AccessType {
    Username,
    Gist,
    Wakatime,
}

fn guard_access(
    config: &AppConfig,
    id: Option<&str>,
    access_type: AccessType,
    options: &RenderOptions,
) -> Option<String> {
    let id = id.unwrap_or_default();
    if id.is_empty() {
        return None;
    }

    match access_type {
        AccessType::Gist => {
            if let Some(wl) = &config.gist_whitelist {
                if !wl.contains(id) {
                    return Some(render_error_card(
                        "This gist ID is not whitelisted",
                        Some("Please deploy your own instance"),
                        options,
                        false,
                    ));
                }
            }
        }
        AccessType::Username | AccessType::Wakatime => {
            if let Some(wl) = &config.whitelist {
                if !wl.contains(id) {
                    return Some(render_error_card(
                        "This username is not whitelisted",
                        Some("Please deploy your own instance"),
                        options,
                        false,
                    ));
                }
            } else if matches!(access_type, AccessType::Username)
                && BLACKLIST.iter().any(|x| x == &id)
            {
                return Some(render_error_card(
                    "This username is blacklisted",
                    Some("Please deploy your own instance"),
                    options,
                    false,
                ));
            }
        }
    }

    None
}

fn q<'a>(params: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    params.get(key).map(String::as_str)
}

fn parse_bool_param(params: &HashMap<String, String>, key: &str) -> Option<bool> {
    q(params, key).and_then(parse_bool)
}

fn parse_i64_param(params: &HashMap<String, String>, key: &str) -> Option<i64> {
    q(params, key).and_then(|v| v.parse::<i64>().ok())
}

fn parse_f64_param(params: &HashMap<String, String>, key: &str) -> Option<f64> {
    q(params, key).and_then(|v| v.parse::<f64>().ok())
}

fn parse_csv_param(params: &HashMap<String, String>, key: &str) -> Vec<String> {
    q(params, key)
        .map(split_csv)
        .unwrap_or_default()
        .into_iter()
        .filter(|v| !v.is_empty())
        .collect()
}

fn split_csv(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_bool(input: &str) -> Option<bool> {
    if input.eq_ignore_ascii_case("true") {
        Some(true)
    } else if input.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

fn render_options_from_query(params: &HashMap<String, String>) -> RenderOptions {
    RenderOptions {
        title_color: q(params, "title_color").map(str::to_string),
        icon_color: q(params, "icon_color").map(str::to_string),
        text_color: q(params, "text_color").map(str::to_string),
        ring_color: q(params, "ring_color").map(str::to_string),
        bg_color: q(params, "bg_color").map(str::to_string),
        border_color: q(params, "border_color").map(str::to_string),
        theme: q(params, "theme").map(str::to_string),
    }
}

fn get_card_colors(options: &RenderOptions, default_theme: &str) -> CardColors {
    let default = THEMES.get("default").expect("default theme must exist");
    let selected = options
        .theme
        .as_ref()
        .and_then(|name| THEMES.get(name))
        .or_else(|| THEMES.get(default_theme))
        .unwrap_or(default);

    let default_border = selected
        .border_color
        .clone()
        .or_else(|| default.border_color.clone())
        .unwrap_or_else(|| "e4e2e2".to_string());

    let title_color = parse_solid_color(
        options
            .title_color
            .as_deref()
            .or_else(|| Some(selected.title_color.as_str())),
        &format!("#{}", default.title_color),
    );

    let icon_color = parse_solid_color(
        options
            .icon_color
            .as_deref()
            .or_else(|| Some(selected.icon_color.as_str())),
        &format!("#{}", default.icon_color),
    );

    let text_color = parse_solid_color(
        options
            .text_color
            .as_deref()
            .or_else(|| Some(selected.text_color.as_str())),
        &format!("#{}", default.text_color),
    );

    let ring_color = parse_solid_color(
        options
            .ring_color
            .as_deref()
            .or_else(|| selected.ring_color.as_deref()),
        &title_color,
    );

    let bg_color = parse_background_color(
        options
            .bg_color
            .as_deref()
            .or_else(|| Some(selected.bg_color.as_str())),
        &format!("#{}", default.bg_color),
    );

    let border_color = parse_solid_color(
        options.border_color.as_deref(),
        &format!("#{default_border}"),
    );

    CardColors {
        title_color,
        icon_color,
        text_color,
        ring_color,
        border_color,
        bg_color,
    }
}

fn parse_solid_color(input: Option<&str>, fallback: &str) -> String {
    input
        .and_then(|value| parse_hex_color(Some(value)))
        .unwrap_or_else(|| fallback.to_string())
}

fn parse_background_color(input: Option<&str>, fallback: &str) -> BackgroundFill {
    if let Some(input) = input {
        let parts: Vec<&str> = input.split(',').collect();
        if parts.len() > 2 {
            let angle = parts[0].trim().to_string();
            let mut gradient_colors = Vec::new();
            for part in &parts[1..] {
                if let Some(color) = parse_hex_color(Some(part.trim())) {
                    gradient_colors.push(color);
                } else {
                    gradient_colors.clear();
                    break;
                }
            }
            if gradient_colors.len() > 1 {
                return BackgroundFill::Gradient {
                    angle,
                    colors: gradient_colors,
                };
            }
        }
        if let Some(color) = parse_hex_color(Some(input)) {
            return BackgroundFill::Solid(color);
        }
    }

    BackgroundFill::Solid(fallback.to_string())
}

fn parse_hex_color(input: Option<&str>) -> Option<String> {
    let raw = input?.trim().trim_start_matches('#');
    let valid_len = matches!(raw.len(), 3 | 4 | 6 | 8);
    if !valid_len {
        return None;
    }
    if !raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{raw}"))
}

async fn graphql_request<T: DeserializeOwned>(
    state: &AppState,
    query: &str,
    variables: Value,
) -> Result<GraphQlEnvelope<T>, AppError> {
    if state.config.tokens.is_empty() {
        return Err(AppError::NoTokens);
    }

    let total = state.config.tokens.len();
    let start = state.next_token.fetch_add(1, AtomicOrdering::Relaxed) % total;

    for offset in 0..total {
        let idx = (start + offset) % total;
        let token = &state.config.tokens[idx];

        let req = state
            .github_client
            .post("https://api.github.com/graphql")
            .bearer_auth(&token.token)
            .json(&json!({ "query": query, "variables": variables }));

        let resp = match req.send().await {
            Ok(resp) => resp,
            Err(err) => {
                error!("graphql send failed with {}: {err}", token.name);
                continue;
            }
        };

        let status = resp.status();
        let text = match resp.text().await {
            Ok(text) => text,
            Err(err) => {
                error!("graphql body read failed with {}: {err}", token.name);
                continue;
            }
        };

        let text_lc = text.to_ascii_lowercase();
        if status == StatusCode::UNAUTHORIZED
            || text_lc.contains("bad credentials")
            || text_lc.contains("account was suspended")
        {
            info!("token {} invalid, trying next", token.name);
            continue;
        }

        if status == StatusCode::FORBIDDEN && text_lc.contains("rate limit") {
            info!("token {} rate-limited, trying next", token.name);
            continue;
        }

        let raw: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(err) => {
                return Err(AppError::Message(format!(
                    "GitHub response parse error: {err}"
                )));
            }
        };

        if let Some(errors) = raw.get("errors").and_then(|v| v.as_array()) {
            let first_type = errors
                .first()
                .and_then(|e| e.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let first_message = errors
                .first()
                .and_then(|e| e.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_ascii_lowercase();

            if first_type == "RATE_LIMITED" || first_message.contains("rate limit") {
                info!("token {} graphql rate-limited, trying next", token.name);
                continue;
            }
        }

        let envelope = serde_json::from_value::<GraphQlEnvelope<T>>(raw)
            .map_err(|e| AppError::Message(format!("GraphQL decode error: {e}")))?;

        return Ok(envelope);
    }

    Err(AppError::MaxRetry)
}

async fn fetch_total_commits(state: &AppState, username: &str) -> Result<i64, AppError> {
    if state.config.tokens.is_empty() {
        return Err(AppError::NoTokens);
    }

    let total = state.config.tokens.len();
    let start = state.next_token.fetch_add(1, AtomicOrdering::Relaxed) % total;

    for offset in 0..total {
        let idx = (start + offset) % total;
        let token = &state.config.tokens[idx];

        let response = state
            .github_client
            .get(format!(
                "https://api.github.com/search/commits?q=author:{username}"
            ))
            .header("Accept", "application/vnd.github.cloak-preview")
            .header("Authorization", format!("token {}", token.token))
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(err) => {
                error!("commits fetch failed {}: {err}", token.name);
                continue;
            }
        };

        let status = response.status();
        let value = match response.json::<Value>().await {
            Ok(v) => v,
            Err(err) => {
                error!("commits json decode failed {}: {err}", token.name);
                continue;
            }
        };

        let message = value
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        if status == StatusCode::UNAUTHORIZED || message.contains("bad credentials") {
            continue;
        }
        if message.contains("rate limit") {
            continue;
        }

        if let Some(total_count) = value.get("total_count").and_then(|v| v.as_i64()) {
            return Ok(total_count);
        }
    }

    Err(AppError::Message(
        "Could not fetch total commits.".to_string(),
    ))
}

async fn fetch_stats(
    state: &AppState,
    username: &str,
    include_all_commits: bool,
    mut exclude_repo: Vec<String>,
    include_merged_prs: bool,
    include_discussions: bool,
    include_discussions_answers: bool,
    commits_year: Option<i64>,
) -> Result<StatsCardData, AppError> {
    if username.trim().is_empty() {
        return Err(AppError::MissingParam("\"username\"".to_string()));
    }

    for repo in &state.config.exclude_repos {
        exclude_repo.push(repo.clone());
    }
    let hidden: HashSet<String> = exclude_repo.into_iter().collect();

    let start_time = commits_year.map(|y| format!("{y}-01-01T00:00:00Z"));
    let variables = json!({
        "login": username,
        "after": Value::Null,
        "includeMergedPullRequests": include_merged_prs,
        "includeDiscussions": include_discussions,
        "includeDiscussionsAnswers": include_discussions_answers,
        "startTime": start_time,
    });

    let first = graphql_request::<StatsData>(state, STATS_QUERY, variables).await?;

    if let Some(errors) = &first.errors {
        return map_graphql_error(errors);
    }

    let mut user = first
        .data
        .and_then(|d| d.user)
        .ok_or_else(|| AppError::Message("Could not fetch user.".to_string()))?;

    if state.config.fetch_multi_page_stars {
        let mut has_next = user
            .repositories
            .page_info
            .as_ref()
            .map(|p| p.has_next_page)
            .unwrap_or(false);
        let mut after = user
            .repositories
            .page_info
            .as_ref()
            .and_then(|p| p.end_cursor.clone());

        while has_next {
            let nodes_with_stars = user
                .repositories
                .nodes
                .iter()
                .filter(|n| n.stargazers.total_count > 0)
                .count();
            if nodes_with_stars != user.repositories.nodes.len() {
                break;
            }

            let vars = json!({ "login": username, "after": after });
            let next =
                graphql_request::<StatsReposOnlyData>(state, STATS_REPOS_QUERY, vars).await?;
            if let Some(errors) = &next.errors {
                return map_graphql_error(errors);
            }

            let Some(data) = next.data.and_then(|d| d.user) else {
                break;
            };

            let repo_conn = data.repositories;
            user.repositories.nodes.extend(repo_conn.nodes);
            has_next = repo_conn
                .page_info
                .as_ref()
                .map(|p| p.has_next_page)
                .unwrap_or(false);
            after = repo_conn.page_info.and_then(|p| p.end_cursor);
        }
    }

    let total_commits = if include_all_commits {
        fetch_total_commits(state, username).await?
    } else {
        user.commits.total_commit_contributions
    };

    let total_prs = user.pull_requests.total_count;
    let total_prs_merged = if include_merged_prs {
        user.merged_pull_requests
            .as_ref()
            .map(|v| v.total_count)
            .unwrap_or(0)
    } else {
        0
    };

    let merged_prs_percentage = if include_merged_prs && total_prs > 0 {
        (total_prs_merged as f64 / total_prs as f64) * 100.0
    } else {
        0.0
    };

    let total_reviews = user.reviews.total_pull_request_review_contributions;
    let total_issues = user.open_issues.total_count + user.closed_issues.total_count;
    let total_discussions_started = if include_discussions {
        user.repository_discussions
            .as_ref()
            .map(|v| v.total_count)
            .unwrap_or(0)
    } else {
        0
    };
    let total_discussions_answered = if include_discussions_answers {
        user.repository_discussion_comments
            .as_ref()
            .map(|v| v.total_count)
            .unwrap_or(0)
    } else {
        0
    };

    let total_stars = user
        .repositories
        .nodes
        .iter()
        .filter(|repo| !hidden.contains(&repo.name))
        .map(|repo| repo.stargazers.total_count)
        .sum::<i64>();

    let name = user.name.unwrap_or_else(|| user.login.clone());

    let rank = calculate_rank(
        include_all_commits,
        total_commits,
        total_prs,
        total_issues,
        total_reviews,
        total_stars,
        user.followers.total_count,
    );

    Ok(StatsCardData {
        name,
        total_prs,
        total_prs_merged,
        merged_prs_percentage,
        total_reviews,
        total_commits,
        total_issues,
        total_stars,
        total_discussions_started,
        total_discussions_answered,
        contributed_to: user.repositories_contributed_to.total_count,
        rank,
    })
}

fn map_graphql_error(errors: &[GraphQlError]) -> Result<StatsCardData, AppError> {
    if let Some(first) = errors.first() {
        if first.kind == "NOT_FOUND" {
            return Err(AppError::UserNotFound);
        }
        if !first.message.is_empty() {
            return Err(AppError::Message(wrap_line(&first.message, 90)));
        }
    }
    Err(AppError::Message(
        "Something went wrong while trying to retrieve the data using the GraphQL API.".to_string(),
    ))
}

async fn fetch_top_languages(
    state: &AppState,
    username: &str,
    mut exclude_repo: Vec<String>,
    size_weight: f64,
    count_weight: f64,
) -> Result<Vec<LangData>, AppError> {
    if username.trim().is_empty() {
        return Err(AppError::MissingParam("\"username\"".to_string()));
    }

    for repo in &state.config.exclude_repos {
        exclude_repo.push(repo.clone());
    }

    let hidden = exclude_repo.into_iter().collect::<HashSet<_>>();

    let response =
        graphql_request::<TopLangData>(state, TOP_LANGS_QUERY, json!({"login": username})).await?;

    if let Some(errors) = &response.errors {
        if let Some(first) = errors.first() {
            if first.kind == "NOT_FOUND" {
                return Err(AppError::UserNotFound);
            }
            if !first.message.is_empty() {
                return Err(AppError::Message(wrap_line(&first.message, 90)));
            }
        }
        return Err(AppError::Message(
            "Something went wrong while trying to retrieve the language data using the GraphQL API.".to_string(),
        ));
    }

    let repos = response
        .data
        .and_then(|d| d.user)
        .map(|u| u.repositories.nodes)
        .unwrap_or_default();

    let mut stats: HashMap<String, (String, f64, f64)> = HashMap::new();

    for repo in repos
        .into_iter()
        .filter(|repo| !hidden.contains(&repo.name))
    {
        for edge in repo.languages.edges {
            if edge.node.name.is_empty() {
                continue;
            }
            let key = edge.node.name.clone();
            let color = edge
                .node
                .color
                .clone()
                .unwrap_or_else(|| "#858585".to_string());
            let entry = stats.entry(key).or_insert((color, 0.0, 0.0));
            entry.1 += edge.size;
            entry.2 += 1.0;
        }
    }

    let mut langs = stats
        .into_iter()
        .map(|(name, (color, size, count))| LangData {
            name,
            color,
            size: size.powf(size_weight.max(0.0)) * count.powf(count_weight.max(0.0)),
        })
        .collect::<Vec<_>>();

    langs.sort_by(|a, b| b.size.partial_cmp(&a.size).unwrap_or(Ordering::Equal));

    Ok(langs)
}

async fn fetch_repo(
    state: &AppState,
    username: &str,
    repo_name: &str,
) -> Result<RepoCardData, AppError> {
    match (username.trim().is_empty(), repo_name.trim().is_empty()) {
        (true, true) => return Err(AppError::MissingParam("\"username\", \"repo\"".to_string())),
        (true, false) => return Err(AppError::MissingParam("\"username\"".to_string())),
        (false, true) => return Err(AppError::MissingParam("\"repo\"".to_string())),
        _ => {}
    }

    let response = graphql_request::<RepoData>(
        state,
        REPO_QUERY,
        json!({"login": username, "repo": repo_name}),
    )
    .await?;

    if let Some(errors) = &response.errors {
        if let Some(first) = errors.first() {
            return Err(AppError::Message(first.message.clone()));
        }
    }

    let data = response
        .data
        .ok_or_else(|| AppError::Message("Not found".to_string()))?;

    let pick = data
        .user
        .as_ref()
        .and_then(|u| u.repository.clone())
        .or_else(|| {
            data.organization
                .as_ref()
                .and_then(|o| o.repository.clone())
        });

    let repo = pick.ok_or_else(|| AppError::Message("Repository not found".to_string()))?;

    if repo.is_private {
        return Err(AppError::Message("Repository not found".to_string()));
    }

    Ok(RepoCardData {
        name: repo.name,
        name_with_owner: repo.name_with_owner,
        description: repo.description,
        primary_language_name: repo.primary_language.as_ref().and_then(|p| p.name.clone()),
        primary_language_color: repo.primary_language.as_ref().and_then(|p| p.color.clone()),
        is_archived: repo.is_archived,
        is_template: repo.is_template,
        star_count: repo.stargazers.total_count,
        fork_count: repo.fork_count,
    })
}

async fn fetch_gist(state: &AppState, id: &str) -> Result<GistCardData, AppError> {
    if id.trim().is_empty() {
        return Err(AppError::MissingParam("\"id\"".to_string()));
    }

    let response = graphql_request::<GistData>(state, GIST_QUERY, json!({"gistName": id})).await?;

    if let Some(errors) = &response.errors {
        if let Some(first) = errors.first() {
            return Err(AppError::Message(first.message.clone()));
        }
    }

    let gist = response
        .data
        .and_then(|d| d.viewer.gist)
        .ok_or_else(|| AppError::Message("Gist not found".to_string()))?;

    let first_file = gist
        .files
        .iter()
        .find(|f| f.name.as_ref().map(|n| !n.is_empty()).unwrap_or(false))
        .cloned();

    let file_name = first_file
        .and_then(|f| f.name)
        .unwrap_or_else(|| "unknown.gist".to_string());

    let mut lang_sizes: HashMap<String, i64> = HashMap::new();
    for file in &gist.files {
        if let Some(lang) = file.language.as_ref().and_then(|l| l.name.clone()) {
            *lang_sizes.entry(lang).or_insert(0) += file.size;
        }
    }

    let primary_language = lang_sizes
        .into_iter()
        .max_by_key(|(_, size)| *size)
        .map(|(name, _)| name);

    Ok(GistCardData {
        name: file_name.clone(),
        name_with_owner: format!("{}/{}", gist.owner.login, file_name),
        description: gist.description,
        language: primary_language,
        stars_count: gist.stargazer_count,
        forks_count: gist.forks.total_count,
    })
}

async fn fetch_wakatime_stats(
    state: &AppState,
    username: &str,
    api_domain: Option<&str>,
) -> Result<WakatimeData, AppError> {
    if username.trim().is_empty() {
        return Err(AppError::MissingParam("\"username\"".to_string()));
    }

    let domain = api_domain.unwrap_or("wakatime.com").trim_end_matches('/');
    let url = format!("https://{domain}/api/v1/users/{username}/stats?is_including_today=true");

    let response = state
        .http_client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Message(format!("WakaTime request failed: {e}")))?;

    if !response.status().is_success() {
        return Err(AppError::WakatimeUserNotFound(username.to_string()));
    }

    let data = response
        .json::<WakatimeResponse>()
        .await
        .map_err(|e| AppError::Message(format!("WakaTime decode failed: {e}")))?;

    Ok(data.data)
}

async fn check_pat_uptime(state: &AppState) -> bool {
    if state.config.tokens.is_empty() {
        return false;
    }

    graphql_request::<Value>(state, RATE_LIMIT_QUERY, json!({}))
        .await
        .is_ok()
}

#[derive(Serialize)]
struct PatInfoResult {
    #[serde(rename = "validPATs")]
    valid_pats: Vec<String>,
    #[serde(rename = "expiredPATs")]
    expired_pats: Vec<String>,
    #[serde(rename = "exhaustedPATs")]
    exhausted_pats: Vec<String>,
    #[serde(rename = "suspendedPATs")]
    suspended_pats: Vec<String>,
    #[serde(rename = "errorPATs")]
    error_pats: Vec<String>,
    details: BTreeMap<String, Value>,
}

async fn get_pat_info(state: &AppState) -> PatInfoResult {
    let mut details: BTreeMap<String, Value> = BTreeMap::new();

    for token in &state.config.tokens {
        let response = state
            .github_client
            .post("https://api.github.com/graphql")
            .bearer_auth(&token.token)
            .json(&json!({"query": RATE_LIMIT_QUERY, "variables": {}}))
            .send()
            .await;

        let Ok(response) = response else {
            details.insert(
                token.name.clone(),
                json!({
                    "status": "error",
                    "error": {"message": "request failed"}
                }),
            );
            continue;
        };

        let status = response.status();
        let value = response.json::<Value>().await.unwrap_or_else(|_| json!({}));

        let message = value
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        if status == StatusCode::UNAUTHORIZED || message == "bad credentials" {
            details.insert(token.name.clone(), json!({"status": "expired"}));
            continue;
        }

        if message.contains("account was suspended") {
            details.insert(token.name.clone(), json!({"status": "suspended"}));
            continue;
        }

        if let Some(errors) = value.get("errors").and_then(|v| v.as_array()) {
            let first = errors.first().cloned().unwrap_or_default();
            let ty = first
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let msg = first
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            if ty == "RATE_LIMITED" || msg.to_ascii_lowercase().contains("rate limit") {
                details.insert(
                    token.name.clone(),
                    json!({"status": "exhausted", "remaining": 0}),
                );
            } else {
                details.insert(
                    token.name.clone(),
                    json!({"status": "error", "error": {"type": ty, "message": msg}}),
                );
            }
            continue;
        }

        let remaining = value
            .get("data")
            .and_then(|d| d.get("rateLimit"))
            .and_then(|r| r.get("remaining"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let reset_at = value
            .get("data")
            .and_then(|d| d.get("rateLimit"))
            .and_then(|r| r.get("resetAt"))
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        if remaining <= 0 {
            let reset_in = reset_at
                .map(|t| ((t - Utc::now()).num_minutes().max(0)).to_string() + " minutes")
                .unwrap_or_else(|| "unknown".to_string());
            details.insert(
                token.name.clone(),
                json!({"status": "exhausted", "remaining": 0, "resetIn": reset_in}),
            );
        } else {
            details.insert(
                token.name.clone(),
                json!({"status": "valid", "remaining": remaining}),
            );
        }
    }

    let status_filter = |status: &str| -> Vec<String> {
        details
            .iter()
            .filter_map(|(pat, info)| {
                if info.get("status").and_then(|v| v.as_str()) == Some(status) {
                    Some(pat.clone())
                } else {
                    None
                }
            })
            .collect()
    };

    PatInfoResult {
        valid_pats: status_filter("valid"),
        expired_pats: status_filter("expired"),
        exhausted_pats: status_filter("exhausted"),
        suspended_pats: status_filter("suspended"),
        error_pats: status_filter("error"),
        details,
    }
}

fn calculate_rank(
    all_commits: bool,
    commits: i64,
    prs: i64,
    issues: i64,
    reviews: i64,
    stars: i64,
    followers: i64,
) -> Rank {
    let commits_median = if all_commits { 1000.0 } else { 250.0 };
    let commits_weight = 2.0;
    let prs_weight = 3.0;
    let issues_weight = 1.0;
    let reviews_weight = 1.0;
    let stars_weight = 4.0;
    let followers_weight = 1.0;

    let total_weight = commits_weight
        + prs_weight
        + issues_weight
        + reviews_weight
        + stars_weight
        + followers_weight;

    let rank = 1.0
        - (commits_weight * exponential_cdf(commits as f64 / commits_median)
            + prs_weight * exponential_cdf(prs as f64 / 50.0)
            + issues_weight * exponential_cdf(issues as f64 / 25.0)
            + reviews_weight * exponential_cdf(reviews as f64 / 2.0)
            + stars_weight * log_normal_cdf(stars as f64 / 50.0)
            + followers_weight * log_normal_cdf(followers as f64 / 10.0))
            / total_weight;

    let percentile = rank * 100.0;

    let thresholds = [1.0, 12.5, 25.0, 37.5, 50.0, 62.5, 75.0, 87.5, 100.0];
    let levels = ["S", "A+", "A", "A-", "B+", "B", "B-", "C+", "C"];

    let mut level = "C";
    for (idx, threshold) in thresholds.iter().enumerate() {
        if percentile <= *threshold {
            level = levels[idx];
            break;
        }
    }

    Rank {
        level: level.to_string(),
        percentile,
    }
}

fn exponential_cdf(x: f64) -> f64 {
    1.0 - 2f64.powf(-x)
}

fn log_normal_cdf(x: f64) -> f64 {
    x / (1.0 + x)
}

fn calculate_circle_progress(value: f64) -> f64 {
    let radius = 40.0;
    let circumference = PI * (radius * 2.0);
    let clamped = value.clamp(0.0, 100.0);
    ((100.0 - clamped) / 100.0) * circumference
}

fn render_stats_card(
    data: &StatsCardData,
    params: &HashMap<String, String>,
    options: &RenderOptions,
) -> String {
    let colors = get_card_colors(options, "default");

    let hide = parse_csv_param(params, "hide")
        .into_iter()
        .collect::<HashSet<_>>();
    let show = parse_csv_param(params, "show")
        .into_iter()
        .collect::<HashSet<_>>();

    let show_icons = parse_bool_param(params, "show_icons").unwrap_or(false);
    let hide_title = parse_bool_param(params, "hide_title").unwrap_or(false);
    let hide_border = parse_bool_param(params, "hide_border").unwrap_or(false);
    let hide_rank = parse_bool_param(params, "hide_rank").unwrap_or(false);
    let text_bold = parse_bool_param(params, "text_bold").unwrap_or(true);
    let disable_animations = parse_bool_param(params, "disable_animations").unwrap_or(false);
    let include_all_commits = parse_bool_param(params, "include_all_commits").unwrap_or(false);
    let commits_year = parse_i64_param(params, "commits_year");
    let number_format = q(params, "number_format")
        .unwrap_or("short")
        .to_ascii_lowercase();
    let number_precision =
        parse_i64_param(params, "number_precision").map(|v| v.clamp(0, 2) as usize);

    let mut stat_items = Vec::new();
    stat_items.push(StatItem {
        id: "stars",
        icon: ICON_STAR,
        label: "Total Stars",
        value: format_number(data.total_stars as f64, &number_format, number_precision),
    });

    let commit_label = if include_all_commits {
        "Total Commits".to_string()
    } else if let Some(y) = commits_year {
        format!("Total Commits ({y})")
    } else {
        "Total Commits (last year)".to_string()
    };

    stat_items.push(StatItem {
        id: "commits",
        icon: ICON_COMMITS,
        label: Box::leak(commit_label.into_boxed_str()),
        value: format_number(data.total_commits as f64, &number_format, number_precision),
    });

    stat_items.push(StatItem {
        id: "prs",
        icon: ICON_PRS,
        label: "Total PRs",
        value: format_number(data.total_prs as f64, &number_format, number_precision),
    });

    if show.contains("prs_merged") {
        stat_items.push(StatItem {
            id: "prs_merged",
            icon: ICON_PRS_MERGED,
            label: "Merged PRs",
            value: format_number(
                data.total_prs_merged as f64,
                &number_format,
                number_precision,
            ),
        });
    }

    if show.contains("prs_merged_percentage") {
        stat_items.push(StatItem {
            id: "prs_merged_percentage",
            icon: ICON_PRS_MERGED_PERCENT,
            label: "Merged PRs %",
            value: format!(
                "{:.*}%",
                number_precision.unwrap_or(2),
                data.merged_prs_percentage
            ),
        });
    }

    if show.contains("reviews") {
        stat_items.push(StatItem {
            id: "reviews",
            icon: ICON_REVIEWS,
            label: "Reviews",
            value: format_number(data.total_reviews as f64, &number_format, number_precision),
        });
    }

    stat_items.push(StatItem {
        id: "issues",
        icon: ICON_ISSUES,
        label: "Issues",
        value: format_number(data.total_issues as f64, &number_format, number_precision),
    });

    if show.contains("discussions_started") {
        stat_items.push(StatItem {
            id: "discussions_started",
            icon: ICON_DISCUSSIONS_STARTED,
            label: "Discussions Started",
            value: format_number(
                data.total_discussions_started as f64,
                &number_format,
                number_precision,
            ),
        });
    }

    if show.contains("discussions_answered") {
        stat_items.push(StatItem {
            id: "discussions_answered",
            icon: ICON_DISCUSSIONS_ANSWERED,
            label: "Discussions Answered",
            value: format_number(
                data.total_discussions_answered as f64,
                &number_format,
                number_precision,
            ),
        });
    }

    stat_items.push(StatItem {
        id: "contribs",
        icon: ICON_CONTRIBS,
        label: "Contributed to",
        value: format_number(data.contributed_to as f64, &number_format, number_precision),
    });

    stat_items.retain(|item| !hide.contains(item.id));

    if stat_items.is_empty() && hide_rank {
        return render_error_card(
            "Could not render stats card.",
            Some("Either stats or rank are required."),
            options,
            false,
        );
    }

    const CARD_MIN_WIDTH: i32 = 287;
    const CARD_DEFAULT_WIDTH: i32 = 287;
    const RANK_CARD_MIN_WIDTH: i32 = 420;
    const RANK_CARD_DEFAULT_WIDTH: i32 = 450;
    const RANK_ONLY_CARD_MIN_WIDTH: i32 = 290;
    const RANK_ONLY_CARD_DEFAULT_WIDTH: i32 = 290;

    let line_height = parse_i64_param(params, "line_height").unwrap_or(25) as i32;

    let icon_width = if show_icons && !stat_items.is_empty() {
        17
    } else {
        0
    };

    let default_title = if stat_items.is_empty() {
        "GitHub Rank"
    } else {
        "GitHub Stats"
    };
    let title = q(params, "custom_title").unwrap_or(default_title);
    let title_width = measure_text(title, 10.0);

    let min_card_width = if hide_rank {
        ((50.0 + title_width * 2.0).max(CARD_MIN_WIDTH as f64)) as i32 + icon_width
    } else if stat_items.is_empty() {
        RANK_ONLY_CARD_MIN_WIDTH + icon_width
    } else {
        RANK_CARD_MIN_WIDTH + icon_width
    };

    let default_width = if hide_rank {
        CARD_DEFAULT_WIDTH + icon_width
    } else if stat_items.is_empty() {
        RANK_ONLY_CARD_DEFAULT_WIDTH + icon_width
    } else {
        RANK_CARD_DEFAULT_WIDTH + icon_width
    };

    let mut width = parse_i64_param(params, "card_width")
        .map(|v| v as i32)
        .unwrap_or(default_width);
    if width < min_card_width {
        width = min_card_width;
    }

    let height = (45 + (stat_items.len() as i32 + 1) * line_height).max(if hide_rank {
        0
    } else if stat_items.is_empty() {
        180
    } else {
        150
    });

    let rank_icon_type = q(params, "rank_icon").unwrap_or("default");

    let mut body = String::with_capacity(4096);

    if !hide_rank {
        let rank_x = if !stat_items.is_empty() {
            let min_x_translation = RANK_CARD_MIN_WIDTH as f64 + icon_width as f64 - 70.0;
            if width > RANK_CARD_DEFAULT_WIDTH {
                let x_max_expansion =
                    min_x_translation + (RANK_CARD_DEFAULT_WIDTH - min_card_width) as f64 / 2.0;
                x_max_expansion + (width - RANK_CARD_DEFAULT_WIDTH) as f64
            } else {
                min_x_translation + (width - min_card_width) as f64 / 2.0
            }
        } else {
            width as f64 / 2.0 + 10.0
        };
        let rank_y = height as f64 / 2.0 - 50.0;

        let rank_inner = match rank_icon_type {
            "github" => format!(
                r#"<svg x="-38" y="-30" height="66" width="66" viewBox="0 0 16 16" data-testid="github-rank-icon"><path d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z"></path></svg>"#
            ),
            "percentile" => format!(
                r#"<text x="-5" y="-12" text-anchor="middle" class="rank-percentile-header">Top</text><text x="-5" y="12" text-anchor="middle" class="rank-percentile-text">{:.1}%</text>"#,
                data.rank.percentile
            ),
            _ => format!(
                r#"<text x="-5" y="12" text-anchor="middle" data-testid="level-rank-icon">{}</text>"#,
                escape_html(&data.rank.level)
            ),
        };
        body.push_str(&format!(
            r#"
      <g data-testid="rank-circle" transform="translate({rank_x}, {rank_y})">
        <circle class="rank-circle-rim" cx="-10" cy="8" r="40" />
        <circle class="rank-circle" cx="-10" cy="8" r="40" />
        <g class="rank-text">{rank_inner}</g>
      </g>
      "#
        ));
    }

    let shift_value_pos = 79.01;
    let mut y = 0;
    for (idx, item) in stat_items.iter().enumerate() {
        let delay = (idx + 3) * 150;
        let icon_svg = if show_icons {
            format!(
                r#"<svg data-testid="icon" class="icon" viewBox="0 0 16 16" version="1.1" width="16" height="16">{}</svg>"#,
                item.icon
            )
        } else {
            String::new()
        };

        let label_x = if show_icons { 25 } else { 0 };
        let value_x = if show_icons { 140.0 } else { 120.0 } + shift_value_pos;

        body.push_str(&format!(
            r#"
      <g class="stagger" style="animation-delay:{delay}ms" transform="translate(25,{y})">
        {icon_svg}
        <text class="stat {}" x="{label_x}" y="12.5">{}:</text>
        <text class="stat {}" x="{value_x}" y="12.5" data-testid="{}">{}</text>
      </g>
      "#,
            if text_bold { "bold" } else { "not_bold" },
            escape_html(item.label),
            if text_bold { "bold" } else { "not_bold" },
            item.id,
            escape_html(&item.value),
        ));

        y += line_height;
    }

    let rank_animation_from = calculate_circle_progress(0.0);
    let rank_animation_to = calculate_circle_progress(100.0 - data.rank.percentile);

    let css = format!(
        r#"
    .stat {{ font: 600 14px 'Segoe UI', Ubuntu, "Helvetica Neue", Sans-Serif; fill: {}; }}
    @supports(-moz-appearance: auto) {{
      .stat {{ font-size:12px; }}
    }}
    .not_bold {{ font-weight: 400; }}
    .bold {{ font-weight: 700; }}
    .icon {{ fill: {}; display: {}; }}
    .stagger {{ opacity:0; animation: fadeInAnimation 0.3s ease-in-out forwards; }}
    .rank-text {{ font: 800 24px 'Segoe UI', Ubuntu, Sans-Serif; fill: {}; animation: scaleInAnimation 0.3s ease-in-out forwards; }}
    .rank-percentile-header {{ font-size:14px; }}
    .rank-percentile-text {{ font-size:16px; }}
    .rank-circle-rim {{ stroke: {}; fill:none; stroke-width:6; opacity:0.2; }}
    .rank-circle {{ stroke: {}; stroke-dasharray:250; fill:none; stroke-width:6; stroke-linecap:round; opacity:0.8; transform-origin:-10px 8px; transform:rotate(-90deg); animation: rankAnimation 1s forwards ease-in-out; }}
    @keyframes rankAnimation {{ from {{ stroke-dashoffset: {}; }} to {{ stroke-dashoffset: {}; }} }}
    "#,
        colors.text_color,
        colors.icon_color,
        if show_icons { "block" } else { "none" },
        colors.text_color,
        colors.ring_color,
        colors.ring_color,
        rank_animation_from,
        rank_animation_to,
    );

    render_card(CardSpec {
        width,
        height,
        hide_title,
        hide_border,
        title: title.to_string(),
        title_prefix_icon: None,
        border_radius: parse_f64_param(params, "border_radius").unwrap_or(4.5),
        disable_animations,
        colors,
        css,
        body,
        accessibility_title: format!("{}, Rank: {}", title, data.rank.level),
        accessibility_desc: format!("GitHub stats for {}", data.name),
    })
}

struct StatItem<'a> {
    id: &'a str,
    icon: &'a str,
    label: &'a str,
    value: String,
}

fn render_top_languages_card(
    top_langs: &[LangData],
    params: &HashMap<String, String>,
    options: &RenderOptions,
) -> String {
    let colors = get_card_colors(options, "default");

    let hide_title = parse_bool_param(params, "hide_title").unwrap_or(false);
    let hide_border = parse_bool_param(params, "hide_border").unwrap_or(false);
    let hide_progress = parse_bool_param(params, "hide_progress").unwrap_or(false);
    let disable_animations = parse_bool_param(params, "disable_animations").unwrap_or(false);
    let layout = q(params, "layout").unwrap_or(if hide_progress { "compact" } else { "normal" });
    let stats_format = q(params, "stats_format").unwrap_or("percentages");

    let mut width = parse_i64_param(params, "card_width")
        .map(|v| v as i32)
        .unwrap_or(300)
        .max(280);

    let hide_langs = parse_csv_param(params, "hide")
        .into_iter()
        .map(|v| v.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    let default_count = if layout == "compact" || hide_progress {
        6
    } else if layout == "donut" {
        5
    } else if layout == "donut-vertical" || layout == "pie" {
        6
    } else {
        5
    };

    let langs_count = parse_i64_param(params, "langs_count")
        .map(|v| v.clamp(1, 20) as usize)
        .unwrap_or(default_count);

    let langs: Vec<LangData> = top_langs
        .iter()
        .filter(|l| !hide_langs.contains(&l.name.to_ascii_lowercase()))
        .take(langs_count)
        .cloned()
        .collect();

    let total_size = langs.iter().map(|l| l.size).sum::<f64>().max(1.0);

    let custom_title = q(params, "custom_title").unwrap_or("Top Languages");
    let border_radius = parse_f64_param(params, "border_radius").unwrap_or(4.5);

    let height;
    let mut body = String::new();

    if langs.is_empty() {
        height = 90;
        let no_data_x = if layout == "pie" || layout == "donut-vertical" {
            25
        } else {
            0
        };
        body.push_str(&format!(
            r#"<text x="{no_data_x}" y="11" class="stat bold" fill="{}">No languages data found</text>"#,
            colors.text_color
        ));
    } else {
        match layout {
            "compact" => {
                height = 90
                    + ((langs.len() as f64 / 2.0).ceil() as i32) * 25
                    + if hide_progress { -25 } else { 0 };
                body.push_str(&render_compact_lang_layout(
                    &langs,
                    total_size,
                    width,
                    hide_progress,
                    stats_format,
                    &colors,
                ));
            }
            "donut" => {
                width += 50;
                height = 215 + (langs.len().saturating_sub(5) as i32) * 32;
                body.push_str(&render_donut_lang_layout(
                    &langs,
                    total_size,
                    width,
                    stats_format,
                ));
            }
            "donut-vertical" => {
                height = 300 + ((langs.len() as f64 / 2.0).ceil() as i32) * 25;
                body.push_str(&render_donut_vertical_layout(
                    &langs,
                    total_size,
                    width,
                    stats_format,
                ));
            }
            "pie" => {
                height = 300 + ((langs.len() as f64 / 2.0).ceil() as i32) * 25;
                body.push_str(&render_pie_layout(&langs, total_size, width, stats_format));
            }
            _ => {
                height = 45 + (langs.len() as i32 + 1) * 40;
                body.push_str(&render_normal_lang_layout(
                    &langs,
                    total_size,
                    width,
                    stats_format,
                    &colors,
                ));
            }
        }
    }

    let css = format!(
        r#"
      .stat {{ font: 600 14px 'Segoe UI', Ubuntu, Sans-Serif; fill: {}; }}
      .lang-name {{ font: 400 11px 'Segoe UI', Ubuntu, Sans-Serif; fill: {}; }}
      .bold {{ font-weight:700; }}
      .stagger {{ opacity:0; animation: fadeInAnimation 0.3s ease-in-out forwards; }}
      @keyframes growWidthAnimation {{ from {{ width: 0; }} to {{ width: 100%; }} }}
      .lang-progress {{ animation: growWidthAnimation 0.6s ease-in-out forwards; }}
    "#,
        colors.text_color, colors.text_color
    );

    render_card(CardSpec {
        width,
        height,
        hide_title,
        hide_border,
        title: custom_title.to_string(),
        title_prefix_icon: None,
        border_radius,
        disable_animations,
        colors,
        css,
        body: if layout == "pie" || layout == "donut-vertical" {
            body
        } else {
            format!(r#"<svg data-testid="lang-items" x="25">{body}</svg>"#)
        },
        accessibility_title: custom_title.to_string(),
        accessibility_desc: "Top languages card".to_string(),
    })
}

fn render_repo_card(
    data: &RepoCardData,
    params: &HashMap<String, String>,
    options: &RenderOptions,
) -> String {
    let colors = get_card_colors(options, "default_repocard");
    let hide_border = parse_bool_param(params, "hide_border").unwrap_or(false);
    let show_owner = parse_bool_param(params, "show_owner").unwrap_or(false);
    let border_radius = parse_f64_param(params, "border_radius").unwrap_or(4.5);

    let max_lines = parse_i64_param(params, "description_lines_count")
        .unwrap_or(3)
        .clamp(1, 3) as usize;
    let desc = parse_emojis(
        data.description
            .as_deref()
            .unwrap_or("No description provided"),
    );
    let wrapped = wrap_text_multiline(&desc, 59, max_lines);

    let description_svg = wrapped
        .iter()
        .map(|line| format!(r#"<tspan dy="1.2em" x="25">{}</tspan>"#, escape_html(line)))
        .collect::<Vec<_>>()
        .join("");

    let header = if show_owner {
        data.name_with_owner.clone()
    } else {
        data.name.clone()
    };

    let header = if header.chars().count() > 35 {
        format!("{}...", header.chars().take(35).collect::<String>())
    } else {
        header
    };

    let description_lines_count = wrapped.len().max(1);
    let height = (if description_lines_count > 1 {
        120
    } else {
        110
    }) + description_lines_count as i32 * 10;

    let lang_name = data
        .primary_language_name
        .clone()
        .unwrap_or_else(|| "Unspecified".to_string());
    let lang_color = data
        .primary_language_color
        .clone()
        .unwrap_or_else(|| "#333".to_string());

    let language_node = create_language_node(&lang_name, &lang_color);
    let stars_node = icon_with_label(
        ICON_STAR,
        &k_formatter(data.star_count as f64, None),
        "stargazers",
        16,
    );
    let forks_node = icon_with_label(
        ICON_FORK,
        &k_formatter(data.fork_count as f64, None),
        "forkcount",
        16,
    );

    let stats_row = flex_row(
        vec![
            (language_node, Some(measure_text(&lang_name, 12.0))),
            (
                stars_node,
                Some(16.0 + measure_text(&k_formatter(data.star_count as f64, None), 12.0)),
            ),
            (
                forks_node,
                Some(16.0 + measure_text(&k_formatter(data.fork_count as f64, None), 12.0)),
            ),
        ],
        25.0,
    );

    let badge = if data.is_template {
        Some("Template")
    } else if data.is_archived {
        Some("Archived")
    } else {
        None
    }
    .map(|label| {
        format!(
            r#"<g data-testid="badge" class="badge" transform="translate(320,-18)"><rect stroke="{}" stroke-width="1" width="70" height="20" x="-12" y="-14" ry="10" rx="10"></rect><text x="23" y="-5" alignment-baseline="central" dominant-baseline="central" text-anchor="middle" fill="{}">{}</text></g>"#,
            colors.text_color,
            colors.text_color,
            escape_html(label)
        )
    })
    .unwrap_or_default();

    let body = format!(
        r#"
      {badge}
      <text class="description" x="25" y="-5">{description_svg}</text>
      <g transform="translate(30,{})">{stats_row}</g>
    "#,
        height - 75
    );

    let css = format!(
        r#"
      .description {{ font: 400 13px 'Segoe UI', Ubuntu, Sans-Serif; fill: {}; }}
      .gray {{ font: 400 12px 'Segoe UI', Ubuntu, Sans-Serif; fill: {}; }}
      .icon {{ fill: {}; }}
      .badge {{ font: 600 11px 'Segoe UI', Ubuntu, Sans-Serif; }}
      .badge rect {{ opacity: 0.2; }}
    "#,
        colors.text_color, colors.text_color, colors.icon_color
    );

    render_card(CardSpec {
        width: 400,
        height,
        hide_title: false,
        hide_border,
        title: header,
        title_prefix_icon: Some(ICON_CONTRIBS),
        border_radius,
        disable_animations: true,
        colors,
        css,
        body,
        accessibility_title: "Repository card".to_string(),
        accessibility_desc: "GitHub repository summary".to_string(),
    })
}

fn render_gist_card(
    data: &GistCardData,
    params: &HashMap<String, String>,
    options: &RenderOptions,
) -> String {
    let colors = get_card_colors(options, "default");
    let hide_border = parse_bool_param(params, "hide_border").unwrap_or(false);
    let show_owner = parse_bool_param(params, "show_owner").unwrap_or(false);
    let border_radius = parse_f64_param(params, "border_radius").unwrap_or(4.5);

    let desc = parse_emojis(
        data.description
            .as_deref()
            .unwrap_or("No description provided"),
    );
    let wrapped = wrap_text_multiline(&desc, 59, 10);
    let description_lines = wrapped.len().max(1);
    let description_svg = wrapped
        .iter()
        .map(|line| format!(r#"<tspan dy="1.2em" x="25">{}</tspan>"#, escape_html(line)))
        .collect::<Vec<_>>()
        .join("");

    let line_height = if description_lines > 3 { 12 } else { 10 };
    let height =
        (if description_lines > 1 { 120 } else { 110 }) + description_lines as i32 * line_height;

    let header = if show_owner {
        data.name_with_owner.clone()
    } else {
        data.name.clone()
    };

    let header = if header.chars().count() > 35 {
        format!("{}...", header.chars().take(35).collect::<String>())
    } else {
        header
    };

    let language = data
        .language
        .clone()
        .unwrap_or_else(|| "Unspecified".to_string());
    let lang_color = LANGUAGE_COLORS
        .get(&language)
        .cloned()
        .unwrap_or_else(|| "#858585".to_string());

    let stats_row = flex_row(
        vec![
            (
                create_language_node(&language, &lang_color),
                Some(measure_text(&language, 12.0)),
            ),
            (
                icon_with_label(
                    ICON_STAR,
                    &k_formatter(data.stars_count as f64, None),
                    "starsCount",
                    16,
                ),
                Some(16.0 + measure_text(&k_formatter(data.stars_count as f64, None), 12.0)),
            ),
            (
                icon_with_label(
                    ICON_FORK,
                    &k_formatter(data.forks_count as f64, None),
                    "forksCount",
                    16,
                ),
                Some(16.0 + measure_text(&k_formatter(data.forks_count as f64, None), 12.0)),
            ),
        ],
        25.0,
    );

    let body = format!(
        r#"
      <text class="description" x="25" y="-5">{description_svg}</text>
      <g transform="translate(30,{})">{stats_row}</g>
    "#,
        height - 75
    );

    let css = format!(
        r#"
      .description {{ font: 400 13px 'Segoe UI', Ubuntu, Sans-Serif; fill: {}; }}
      .gray {{ font: 400 12px 'Segoe UI', Ubuntu, Sans-Serif; fill: {}; }}
      .icon {{ fill: {}; }}
    "#,
        colors.text_color, colors.text_color, colors.icon_color
    );

    render_card(CardSpec {
        width: 400,
        height,
        hide_title: false,
        hide_border,
        title: header,
        title_prefix_icon: Some(ICON_GIST),
        border_radius,
        disable_animations: false,
        colors,
        css,
        body,
        accessibility_title: "Gist card".to_string(),
        accessibility_desc: "GitHub gist summary".to_string(),
    })
}

fn render_wakatime_card(
    data: &WakatimeData,
    params: &HashMap<String, String>,
    options: &RenderOptions,
) -> String {
    let colors = get_card_colors(options, "default");

    let hide_title = parse_bool_param(params, "hide_title").unwrap_or(false);
    let hide_border = parse_bool_param(params, "hide_border").unwrap_or(false);
    let hide_progress = parse_bool_param(params, "hide_progress").unwrap_or(false);
    let disable_animations = parse_bool_param(params, "disable_animations").unwrap_or(false);
    let layout = q(params, "layout").unwrap_or("normal");
    let display_format = q(params, "display_format").unwrap_or("time");
    let line_height = parse_i64_param(params, "line_height").unwrap_or(25) as i32;

    let mut width = parse_i64_param(params, "card_width")
        .map(|v| v as i32)
        .unwrap_or(495);
    if layout == "compact" {
        width = width.max(400);
    } else {
        width = width.max(250);
    }

    let hide = parse_csv_param(params, "hide")
        .into_iter()
        .map(|v| v.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    let langs_count = parse_i64_param(params, "langs_count")
        .map(|v| v.max(1) as usize)
        .unwrap_or(data.languages.len().max(1));

    let mut languages: Vec<WakatimeLanguage> = data
        .languages
        .iter()
        .filter(|lang| !hide.contains(&lang.name.to_ascii_lowercase()))
        .take(langs_count)
        .cloned()
        .collect();

    let total_percent = languages.iter().map(|l| l.percent).sum::<f64>();
    if total_percent > 0.0 {
        let weight = ((100.0 / total_percent) * 100.0).round() / 100.0;
        for lang in &mut languages {
            lang.percent = ((lang.percent * weight) * 100.0).round() / 100.0;
        }
    }

    let custom_title = q(params, "custom_title").unwrap_or("WakaTime Stats");
    let border_radius = parse_f64_param(params, "border_radius").unwrap_or(4.5);

    let mut body = String::new();
    let height;

    if layout == "compact" {
        height = 90 + ((languages.len() as f64 / 2.0).ceil() as i32) * 25;

        if !hide_progress {
            let compact_width = width as f64 - 5.0;
            let mut offset = 0.0;
            body.push_str(&format!(
                r#"<mask id="rect-mask"><rect x="25" y="0" width="{}" height="8" fill="white" rx="5"/></mask>"#,
                compact_width - 50.0
            ));
            for lang in &languages {
                let color = LANGUAGE_COLORS
                    .get(&lang.name)
                    .cloned()
                    .unwrap_or_else(|| "#858585".to_string());
                let w = ((compact_width - 25.0) * lang.percent) / 100.0;
                body.push_str(&format!(
                    r#"<rect mask="url(#rect-mask)" x="{offset}" y="0" width="{w}" height="8" fill="{color}" data-testid="lang-progress" />"#
                ));
                offset += w;
            }
        }

        if languages.is_empty() {
            body.push_str(&no_coding_activity_text(data, &colors));
        } else {
            let left_x = 25;
            let right_x = 230 + ((width - 495) / 2);
            for (idx, lang) in languages.iter().enumerate() {
                let is_left = idx % 2 == 0;
                let x = if is_left { left_x } else { right_x };
                let y = 25 + ((idx / 2) as i32) * 25;
                body.push_str(&create_compact_waka_lang(
                    lang,
                    x,
                    y,
                    display_format,
                    &colors,
                ));
            }
        }
    } else {
        height = (45 + (languages.len() as i32 + 1) * line_height).max(150);

        if languages.is_empty() {
            body.push_str(&no_coding_activity_text(data, &colors));
        } else {
            let progress_width = (width - 275).max(80);
            let mut y = 0;
            for (idx, lang) in languages.iter().enumerate() {
                let delay = (idx + 3) * 150;
                let value = if display_format == "percent" {
                    format!("{:.2} %", lang.percent)
                } else {
                    lang.text.clone()
                };
                let bar = if hide_progress {
                    String::new()
                } else {
                    create_progress_node(
                        110,
                        4,
                        progress_width,
                        &colors.title_color,
                        lang.percent,
                        &colors.text_color,
                        delay as i32 + 300,
                    )
                };

                let value_x = if hide_progress {
                    170
                } else {
                    130 + progress_width
                };

                body.push_str(&format!(
                    r#"
                  <g class="stagger" style="animation-delay:{delay}ms" transform="translate(25,{y})">
                    <text class="stat bold" y="12.5" data-testid="{}">{}:</text>
                    <text class="stat" x="{}" y="12.5">{}</text>
                    {}
                  </g>
                "#,
                    escape_html(&lang.name),
                    escape_html(&lang.name),
                    value_x,
                    escape_html(&value),
                    bar,
                ));
                y += line_height;
            }
        }
    }

    let mut title = custom_title.to_string();
    match data.range.as_deref() {
        Some("last_7_days") => title.push_str(" (last 7 days)"),
        Some("last_year") => title.push_str(" (last year)"),
        _ => {}
    }

    let css = format!(
        r#"
      .stat {{ font: 600 14px 'Segoe UI', Ubuntu, Sans-Serif; fill: {}; }}
      .lang-name {{ font: 400 11px 'Segoe UI', Ubuntu, Sans-Serif; fill: {}; }}
      .bold {{ font-weight:700; }}
      .stagger {{ opacity:0; animation: fadeInAnimation 0.3s ease-in-out forwards; }}
      @keyframes growWidthAnimation {{ from {{ width: 0; }} to {{ width: 100%; }} }}
      .lang-progress {{ animation: growWidthAnimation 0.6s ease-in-out forwards; }}
    "#,
        colors.text_color, colors.text_color
    );

    render_card(CardSpec {
        width,
        height,
        hide_title,
        hide_border,
        title,
        title_prefix_icon: None,
        border_radius,
        disable_animations,
        colors,
        css,
        body: format!(r#"<svg x="0" y="0" width="100%">{body}</svg>"#),
        accessibility_title: "WakaTime card".to_string(),
        accessibility_desc: "WakaTime coding activity".to_string(),
    })
}

fn render_error_card(
    message: &str,
    secondary_message: Option<&str>,
    options: &RenderOptions,
    show_repo_link: bool,
) -> String {
    let colors = get_card_colors(options, "default");
    let secondary = secondary_message.unwrap_or("");

    let repo_hint = if show_repo_link
        && !secondary.eq_ignore_ascii_case("Please try again later")
        && !secondary.eq_ignore_ascii_case(
            "You can deploy own instance or wait until public will be no longer limited",
        ) {
        " file an issue at https://tiny.one/readme-stats"
    } else {
        ""
    };

    format!(
        r#"
    <svg width="576.5" height="120" viewBox="0 0 576.5 120" fill="{}" xmlns="http://www.w3.org/2000/svg">
      <style>
        .text {{ font: 600 16px 'Segoe UI', Ubuntu, Sans-Serif; fill: {}; }}
        .small {{ font: 600 12px 'Segoe UI', Ubuntu, Sans-Serif; fill: {}; }}
        .gray {{ fill: #858585; }}
      </style>
      <rect x="0.5" y="0.5" width="575.5" height="99%" rx="4.5" fill="{}" stroke="{}"/>
      <text x="25" y="45" class="text">Something went wrong!{}</text>
      <text x="25" y="55" class="text small">
        <tspan x="25" dy="18">{}</tspan>
        <tspan x="25" dy="18" class="gray">{}</tspan>
      </text>
    </svg>
  "#,
        background_fill_value(&colors.bg_color),
        colors.title_color,
        colors.text_color,
        background_fill_value(&colors.bg_color),
        colors.border_color,
        repo_hint,
        escape_html(message),
        escape_html(secondary),
    )
}

struct CardSpec {
    width: i32,
    height: i32,
    hide_title: bool,
    hide_border: bool,
    title: String,
    title_prefix_icon: Option<&'static str>,
    border_radius: f64,
    disable_animations: bool,
    colors: CardColors,
    css: String,
    body: String,
    accessibility_title: String,
    accessibility_desc: String,
}

fn render_card(card: CardSpec) -> String {
    let gradient = match &card.colors.bg_color {
        BackgroundFill::Gradient { angle, colors } => {
            let stops = colors
                .iter()
                .enumerate()
                .map(|(i, color)| {
                    let offset = if colors.len() <= 1 {
                        0.0
                    } else {
                        (i as f64 * 100.0) / (colors.len() - 1) as f64
                    };
                    format!(r#"<stop offset="{offset}%" stop-color="{color}"/>"#)
                })
                .collect::<Vec<_>>()
                .join("");
            format!(
                r#"<defs><linearGradient id="gradient" gradientTransform="rotate({})" gradientUnits="userSpaceOnUse">{}</linearGradient></defs>"#,
                escape_html(angle),
                stops
            )
        }
        BackgroundFill::Solid(_) => String::new(),
    };

    let fill_value = background_fill_value(&card.colors.bg_color);

    let title_svg = if card.hide_title {
        String::new()
    } else {
        let prefix = if let Some(icon) = card.title_prefix_icon {
            format!(
                r#"<svg class="icon" x="0" y="-13" viewBox="0 0 16 16" width="16" height="16">{icon}</svg>"#
            )
        } else {
            String::new()
        };

        let title_text_x = if card.title_prefix_icon.is_some() {
            25
        } else {
            0
        };

        format!(
            r#"
          <g data-testid="card-title" transform="translate(25,35)">
            {prefix}
            <text x="{title_text_x}" y="0" class="header" data-testid="header">{}</text>
          </g>
        "#,
            escape_html(&card.title)
        )
    };

    let animations_disable = if card.disable_animations {
        "* { animation-duration: 0s !important; animation-delay: 0s !important; }"
    } else {
        ""
    };

    let rendered_height = if card.hide_title {
        (card.height - 30).max(1)
    } else {
        card.height
    };
    let main_translate_y = if card.hide_title { 25 } else { 55 };

    format!(
        r#"
      <svg width="{}" height="{}" viewBox="0 0 {} {}" fill="none" xmlns="http://www.w3.org/2000/svg" role="img" aria-labelledby="titleId descId">
        <title id="titleId">{}</title>
        <desc id="descId">{}</desc>
        <style>
          .header {{ font: 600 18px 'Segoe UI', Ubuntu, Sans-Serif; fill: {}; animation: fadeInAnimation 0.8s ease-in-out forwards; }}
          @supports(-moz-appearance: auto) {{
            .header {{ font-size: 15.5px; }}
          }}
          .icon {{ fill: {}; }}
          {}
          @keyframes scaleInAnimation {{ from {{ transform: translate(-5px,5px) scale(0); }} to {{ transform: translate(-5px,5px) scale(1); }} }}
          @keyframes fadeInAnimation {{ from {{ opacity:0; }} to {{ opacity:1; }} }}
          {}
        </style>
        {}
        <rect data-testid="card-bg" x="0.5" y="0.5" rx="{}" height="99%" stroke="{}" width="{}" fill="{}" stroke-opacity="{}"/>
        {}
        <g data-testid="main-card-body" transform="translate(0,{})">{}
        </g>
      </svg>
    "#,
        card.width,
        rendered_height,
        card.width,
        rendered_height,
        escape_html(&card.accessibility_title),
        escape_html(&card.accessibility_desc),
        card.colors.title_color,
        card.colors.icon_color,
        card.css,
        animations_disable,
        gradient,
        card.border_radius,
        card.colors.border_color,
        card.width - 1,
        fill_value,
        if card.hide_border { 0 } else { 1 },
        title_svg,
        main_translate_y,
        card.body,
    )
}

fn background_fill_value(bg: &BackgroundFill) -> String {
    match bg {
        BackgroundFill::Solid(color) => color.clone(),
        BackgroundFill::Gradient { .. } => "url(#gradient)".to_string(),
    }
}

fn render_normal_lang_layout(
    langs: &[LangData],
    total_size: f64,
    width: i32,
    stats_format: &str,
    colors: &CardColors,
) -> String {
    let mut out = String::new();
    let progress_width = (width - 95).max(80);
    let progress_text_x = width - 85;

    for (idx, lang) in langs.iter().enumerate() {
        let delay = (idx + 3) * 150;
        let pct = ((lang.size / total_size) * 100.0).clamp(0.0, 100.0);
        let value = if stats_format == "bytes" {
            format_bytes(lang.size)
        } else {
            format!("{pct:.2}%")
        };

        out.push_str(&format!(
            r#"
          <g class="stagger" style="animation-delay:{delay}ms" transform="translate(0,{})">
            <text data-testid="lang-name" x="2" y="15" class="lang-name">{}</text>
            <text x="{}" y="34" class="lang-name">{}</text>
            {}
          </g>
        "#,
            idx as i32 * 40,
            escape_html(&lang.name),
            progress_text_x,
            escape_html(&value),
            create_progress_node(
                0,
                25,
                progress_width,
                &lang.color,
                pct,
                "#ddd",
                delay as i32 + 300,
            ),
        ));
    }

    if out.is_empty() {
        out.push_str(&format!(
            r#"<text x="0" y="11" class="stat bold" fill="{}">No languages data found</text>"#,
            colors.text_color
        ));
    }

    out
}

fn render_compact_lang_layout(
    langs: &[LangData],
    total_size: f64,
    width: i32,
    hide_progress: bool,
    stats_format: &str,
    _colors: &CardColors,
) -> String {
    let mut out = String::new();
    let offset_width = width - 50;

    if !hide_progress {
        let mut progress_offset = 0.0;
        out.push_str(&format!(
            r#"<mask id="rect-mask"><rect x="0" y="0" width="{offset_width}" height="8" fill="white" rx="5"/></mask>"#
        ));

        for lang in langs {
            let percentage =
                (((lang.size / total_size) * offset_width as f64) * 100.0).round() / 100.0;
            let section = if percentage < 10.0 {
                percentage + 10.0
            } else {
                percentage
            };
            out.push_str(&format!(
                r#"<rect mask="url(#rect-mask)" data-testid="lang-progress" x="{}" y="0" width="{}" height="8" fill="{}"/>"#,
                progress_offset, section, lang.color
            ));
            progress_offset += percentage;
        }
    }

    let y_start = if hide_progress { 0 } else { 25 };

    let chunk = ((langs.len() as f64 / 2.0).ceil() as usize).max(1);
    let (left, right) = langs.split_at(chunk.min(langs.len()));

    let left_svg = left
        .iter()
        .enumerate()
        .map(|(idx, lang)| compact_lang_node(lang, total_size, hide_progress, stats_format, idx))
        .collect::<Vec<_>>()
        .join("");

    let right_svg = right
        .iter()
        .enumerate()
        .map(|(idx, lang)| compact_lang_node(lang, total_size, hide_progress, stats_format, idx))
        .collect::<Vec<_>>()
        .join("");

    let max_gap = longest_lang(langs)
        .map(|lang| {
            let percent = (lang.size / total_size) * 100.0;
            20.0 + measure_text(&format!("{} {:.2}%", lang.name, percent), 11.0)
        })
        .unwrap_or(150.0);
    let gap = max_gap.max(150.0);

    out.push_str(&format!(
        r#"<g transform="translate(0,{y_start})"><g transform="translate(0,0)">{left_svg}</g><g transform="translate({gap},0)">{right_svg}</g></g>"#
    ));

    out
}

fn longest_lang(langs: &[LangData]) -> Option<&LangData> {
    langs.iter().max_by_key(|l| l.name.len())
}

fn compact_lang_node(
    lang: &LangData,
    total_size: f64,
    hide_progress: bool,
    stats_format: &str,
    index: usize,
) -> String {
    let percent = ((lang.size / total_size) * 100.0).clamp(0.0, 100.0);
    let value = if stats_format == "bytes" {
        format_bytes(lang.size)
    } else {
        format!("{percent:.2}%")
    };

    let delay = (index + 3) * 150;

    format!(
        r#"<g class="stagger" style="animation-delay:{delay}ms" transform="translate(0,{})"><circle cx="5" cy="6" r="5" fill="{}" /><text data-testid="lang-name" x="15" y="10" class="lang-name">{} {}</text></g>"#,
        index as i32 * 25,
        lang.color,
        escape_html(&lang.name),
        if hide_progress {
            String::new()
        } else {
            escape_html(&value)
        }
    )
}

fn render_donut_lang_layout(
    langs: &[LangData],
    total_size: f64,
    width: i32,
    stats_format: &str,
) -> String {
    let center_x = width as f64 / 3.0;
    let center_y = width as f64 / 3.0;
    let radius = center_x - 60.0;
    let stroke_width = 12.0;

    let mut start_angle = 0.0;
    let mut paths = String::new();

    if langs.len() == 1 {
        let color = &langs[0].color;
        paths.push_str(&format!(
            r#"<circle cx="{center_x}" cy="{center_y}" r="{radius}" stroke="{color}" fill="none" stroke-width="{stroke_width}" data-testid="lang-donut" size="100"/>"#
        ));
    } else {
        for (idx, lang) in langs.iter().enumerate() {
            let percent = ((lang.size / total_size) * 100.0).clamp(0.0, 100.0);
            let end_angle = start_angle + (percent / 100.0) * 360.0;
            let start = polar_to_cartesian(center_x, center_y, radius, end_angle - 90.0);
            let end = polar_to_cartesian(center_x, center_y, radius, start_angle - 90.0);
            let large_arc = if end_angle - start_angle <= 180.0 {
                0
            } else {
                1
            };
            let delay = (idx + 3) * 100 + 300;

            paths.push_str(&format!(
                r#"<g class="stagger" style="animation-delay:{delay}ms"><path data-testid="lang-donut" size="{percent:.2}" d="M {} {} A {} {} 0 {} 0 {} {}" stroke="{}" fill="none" stroke-width="{}"></path></g>"#,
                start.0, start.1, radius, radius, large_arc, end.0, end.1, lang.color, stroke_width
            ));

            start_angle = end_angle;
        }
    }

    let legend = langs
        .iter()
        .enumerate()
        .map(|(idx, lang)| {
            let y = idx as i32 * 32;
            let percent = ((lang.size / total_size) * 100.0).clamp(0.0, 100.0);
            let value = if stats_format == "bytes" {
                format_bytes(lang.size)
            } else {
                format!("{percent:.2}%")
            };
            format!(
                r#"<g transform="translate(0,{y})"><circle cx="5" cy="6" r="5" fill="{}"/><text class="lang-name" x="15" y="10">{} {}</text></g>"#,
                lang.color,
                escape_html(&lang.name),
                escape_html(&value),
            )
        })
        .collect::<Vec<_>>()
        .join("");

    format!(
        r#"<g transform="translate(0,0)"><g transform="translate(0,0)">{legend}</g><g transform="translate(125,{})"><svg width="{width}" height="{width}">{paths}</svg></g></g>"#,
        -45 + (langs.len().saturating_sub(5) as i32 * 16)
    )
}

fn render_donut_vertical_layout(
    langs: &[LangData],
    total_size: f64,
    _width: i32,
    stats_format: &str,
) -> String {
    let radius = 80.0;
    let circumference = 2.0 * PI * radius;

    let mut circles = String::new();
    let mut offset = 0.0;

    for (idx, lang) in langs.iter().enumerate() {
        let pct = (lang.size / total_size) * 100.0;
        let len = circumference * (pct / 100.0);
        let delay = (idx + 1) * 100;

        circles.push_str(&format!(
            r#"<g class="stagger" style="animation-delay:{delay}ms"><circle cx="150" cy="100" r="{radius}" fill="transparent" stroke="{}" stroke-width="25" stroke-dasharray="{circumference}" stroke-dashoffset="{offset}" size="{pct:.2}" data-testid="lang-donut"/></g>"#,
            lang.color
        ));
        offset += len;
    }

    let legends = compact_legend_nodes(langs, total_size, stats_format);

    format!(
        r#"<svg data-testid="lang-items"><g transform="translate(0,0)"><svg data-testid="donut">{circles}</svg></g><g transform="translate(0,220)"><svg data-testid="lang-names" x="25">{legends}</svg></g></svg>"#
    )
}

fn render_pie_layout(
    langs: &[LangData],
    total_size: f64,
    _width: i32,
    stats_format: &str,
) -> String {
    let radius = 90.0;
    let cx = 150.0;
    let cy = 100.0;
    let mut start_angle = 0.0;

    let mut paths = String::new();

    if langs.len() == 1 {
        paths.push_str(&format!(
            r#"<circle cx="{cx}" cy="{cy}" r="{radius}" fill="{}" data-testid="lang-pie" size="100"/>"#,
            langs[0].color
        ));
    } else {
        for (idx, lang) in langs.iter().enumerate() {
            let part = lang.size / total_size;
            let pct = part * 100.0;
            let angle = part * 360.0;
            let end_angle = start_angle + angle;

            let start = polar_to_cartesian(cx, cy, radius, start_angle);
            let end = polar_to_cartesian(cx, cy, radius, end_angle);
            let large_arc = if angle > 180.0 { 1 } else { 0 };
            let delay = (idx + 1) * 100;

            paths.push_str(&format!(
                r#"<g class="stagger" style="animation-delay:{delay}ms"><path data-testid="lang-pie" size="{pct:.2}" d="M {cx} {cy} L {} {} A {radius} {radius} 0 {large_arc} 1 {} {} Z" fill="{}" /></g>"#,
                start.0,
                start.1,
                end.0,
                end.1,
                lang.color,
            ));

            start_angle = end_angle;
        }
    }

    let legends = compact_legend_nodes(langs, total_size, stats_format);

    format!(
        r#"<svg data-testid="lang-items"><g transform="translate(0,0)"><svg data-testid="pie">{paths}</svg></g><g transform="translate(0,220)"><svg data-testid="lang-names" x="25">{legends}</svg></g></svg>"#
    )
}

fn compact_legend_nodes(langs: &[LangData], total_size: f64, stats_format: &str) -> String {
    let half = ((langs.len() as f64 / 2.0).ceil() as usize).max(1);
    let (left, right) = langs.split_at(half.min(langs.len()));

    let col = |slice: &[LangData]| {
        slice
            .iter()
            .enumerate()
            .map(|(idx, lang)| {
                let y = idx as i32 * 25;
                let pct = ((lang.size / total_size) * 100.0).clamp(0.0, 100.0);
                let value = if stats_format == "bytes" {
                    format_bytes(lang.size)
                } else {
                    format!("{pct:.2}%")
                };
                format!(
                    r#"<g transform="translate(0,{y})"><circle cx="5" cy="6" r="5" fill="{}" /><text x="15" y="10" class="lang-name">{} {}</text></g>"#,
                    lang.color,
                    escape_html(&lang.name),
                    escape_html(&value),
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };

    let left_svg = col(left);
    let right_svg = col(right);

    format!(
        r#"<g transform="translate(0,0)">{left_svg}</g><g transform="translate(180,0)">{right_svg}</g>"#
    )
}

fn polar_to_cartesian(cx: f64, cy: f64, radius: f64, angle_deg: f64) -> (f64, f64) {
    let rads = angle_deg * PI / 180.0;
    (cx + radius * rads.cos(), cy + radius * rads.sin())
}

fn create_compact_waka_lang(
    lang: &WakatimeLanguage,
    x: i32,
    y: i32,
    display_format: &str,
    _colors: &CardColors,
) -> String {
    let color = LANGUAGE_COLORS
        .get(&lang.name)
        .cloned()
        .unwrap_or_else(|| "#858585".to_string());
    let value = if display_format == "percent" {
        format!("{:.2} %", lang.percent)
    } else {
        lang.text.clone()
    };

    format!(
        r#"<g transform="translate({x},{y})"><circle cx="5" cy="6" r="5" fill="{color}" /><text data-testid="lang-name" x="15" y="10" class="lang-name">{} - {}</text></g>"#,
        escape_html(&lang.name),
        escape_html(&value)
    )
}

fn no_coding_activity_text(data: &WakatimeData, colors: &CardColors) -> String {
    let text = match (
        data.is_coding_activity_visible.unwrap_or(true),
        data.is_other_usage_visible.unwrap_or(true),
    ) {
        (false, _) => "WakaTime profile is not public",
        (true, false) => "No coding details found",
        _ => "No coding activity found",
    };

    format!(
        r#"<text x="25" y="11" class="stat bold" fill="{}">{}</text>"#,
        colors.text_color,
        escape_html(text)
    )
}

fn create_progress_node(
    x: i32,
    y: i32,
    width: i32,
    color: &str,
    progress: f64,
    bg: &str,
    delay_ms: i32,
) -> String {
    let progress_pct = progress.clamp(2.0, 100.0);
    format!(
        r#"<svg width="{width}" x="{x}" y="{y}"><rect rx="5" ry="5" x="0" y="0" width="{width}" height="8" fill="{bg}"></rect><svg data-testid="lang-progress" width="{progress_pct}%"><rect height="8" fill="{color}" rx="5" ry="5" x="0" y="0" class="lang-progress" style="animation-delay:{delay_ms}ms;"/></svg></svg>"#
    )
}

fn create_language_node(name: &str, color: &str) -> String {
    format!(
        r#"<g data-testid="primary-lang"><circle data-testid="lang-color" cx="0" cy="-5" r="6" fill="{}" /><text data-testid="lang-name" class="gray" x="15">{}</text></g>"#,
        color,
        escape_html(name)
    )
}

fn icon_with_label(icon: &str, label: &str, testid: &str, icon_size: i32) -> String {
    if label == "0" {
        return String::new();
    }

    let icon_svg = format!(
        r#"<svg class="icon" y="-12" viewBox="0 0 16 16" version="1.1" width="{icon_size}" height="{icon_size}">{icon}</svg>"#
    );

    let text = format!(
        r#"<text data-testid="{}" class="gray">{}</text>"#,
        testid,
        escape_html(label)
    );

    flex_row(vec![(icon_svg, None), (text, None)], 20.0)
}

fn flex_row(items: Vec<(String, Option<f64>)>, gap: f64) -> String {
    let mut out = String::new();
    let mut offset = 0.0;
    for (item, size) in items.into_iter().filter(|(s, _)| !s.is_empty()) {
        out.push_str(&format!(
            r#"<g transform="translate({offset},0)">{item}</g>"#
        ));
        offset += size.unwrap_or(0.0) + gap;
    }
    out
}

const TEXT_WIDTHS: &[f64] = &[
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.2796875, 0.2765625,
    0.3546875, 0.5546875, 0.5546875, 0.8890625, 0.665625, 0.190625, 0.3328125, 0.3328125,
    0.3890625, 0.5828125, 0.2765625, 0.3328125, 0.2765625, 0.3015625, 0.5546875, 0.5546875,
    0.5546875, 0.5546875, 0.5546875, 0.5546875, 0.5546875, 0.5546875, 0.5546875, 0.5546875,
    0.2765625, 0.2765625, 0.584375, 0.5828125, 0.584375, 0.5546875, 1.0140625, 0.665625, 0.665625,
    0.721875, 0.721875, 0.665625, 0.609375, 0.7765625, 0.721875, 0.2765625, 0.5, 0.665625,
    0.5546875, 0.8328125, 0.721875, 0.7765625, 0.665625, 0.7765625, 0.721875, 0.665625, 0.609375,
    0.721875, 0.665625, 0.94375, 0.665625, 0.665625, 0.609375, 0.2765625, 0.3546875, 0.2765625,
    0.4765625, 0.5546875, 0.3328125, 0.5546875, 0.5546875, 0.5, 0.5546875, 0.5546875, 0.2765625,
    0.5546875, 0.5546875, 0.221875, 0.240625, 0.5, 0.221875, 0.8328125, 0.5546875, 0.5546875,
    0.5546875, 0.5546875, 0.3328125, 0.5, 0.2765625, 0.5546875, 0.5, 0.721875, 0.5, 0.5, 0.5,
    0.3546875, 0.259375, 0.353125, 0.5890625,
];
const TEXT_WIDTH_AVG: f64 = 0.5279276315789471;

fn measure_text(text: &str, font_size: f64) -> f64 {
    text.chars()
        .map(|ch| {
            TEXT_WIDTHS
                .get(ch as usize)
                .copied()
                .unwrap_or(TEXT_WIDTH_AVG)
        })
        .sum::<f64>()
        * font_size
}

fn k_formatter(num: f64, precision: Option<usize>) -> String {
    let abs = num.abs();
    let sign = if num < 0.0 { -1.0 } else { 1.0 };

    if let Some(precision) = precision {
        return format!("{:.*}k", precision, sign * (abs / 1000.0));
    }

    if abs < 1000.0 {
        if abs.fract() == 0.0 {
            format!("{}", (sign * abs) as i64)
        } else {
            format!("{}", sign * abs)
        }
    } else {
        let v = (abs / 1000.0 * 10.0).round() / 10.0;
        format!("{}k", sign * v)
    }
}

fn format_number(num: f64, number_format: &str, precision: Option<usize>) -> String {
    if number_format == "long" {
        if num.fract() == 0.0 {
            format!("{}", num as i64)
        } else {
            format!("{}", num)
        }
    } else {
        k_formatter(num, precision)
    }
}

fn format_bytes(bytes: f64) -> String {
    if bytes <= 0.0 {
        return "0 B".to_string();
    }

    let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    let i = (bytes.log(1024.0).floor() as usize).min(units.len() - 1);
    format!("{:.1} {}", bytes / 1024f64.powi(i as i32), units[i])
}

fn wrap_text_multiline(text: &str, width: usize, max_lines: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.len() + word.len() + 1 > width {
            if !current.is_empty() {
                lines.push(current.trim().to_string());
                current.clear();
                if lines.len() >= max_lines {
                    break;
                }
            }
        }
        current.push_str(word);
        current.push(' ');
    }

    if lines.len() < max_lines && !current.trim().is_empty() {
        lines.push(current.trim().to_string());
    }

    if lines.len() > max_lines {
        lines.truncate(max_lines);
    }

    lines
}

fn wrap_line(text: &str, width: usize) -> String {
    if text.len() <= width {
        return text.to_string();
    }
    text.chars().take(width).collect::<String>()
}

fn parse_emojis(input: &str) -> String {
    input.to_string()
}

fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}
