# github-readme-stats-rs

A Rust port of [github-readme-stats](https://github.com/anuraghazra/github-readme-stats), following its successor [GitHub Stats Extended](https://github.com/stats-organization/github-stats-extended). Built with [axum](https://github.com/tokio-rs/axum), it operates as a high-performance single binary without requiring Docker.

## Features

- **Asynchronous I/O**: Powered by `axum` + `tokio` + `reqwest`.
- **API Compatibility**: Supports `/api`, `/api/top-langs`, `/api/pin`, `/api/gist`, and `/api/wakatime` routes.
- **Status Endpoints**: Implements `/api/status/up` and `/api/status/pat-info`.
- **Efficiency**: Connection pooling, PAT (Personal Access Token) rotation, and in-app response caching.
- **Optimized Binary**: Release builds utilize `LTO`, `codegen-units=1`, `panic=abort`, and symbol stripping for minimal footprint.

## Getting Started

```bash
cargo run --release

```

The server listens on `0.0.0.0:8080` by default.

## Environment Variables

| Variable                 | Description                                               |
| ------------------------ | --------------------------------------------------------- |
| `PAT_1`, `PAT_2`, ...    | GitHub Personal Access Tokens for rotation.               |
| `GITHUB_TOKEN`           | Fallback token if no `PAT_*` variables are defined.       |
| `HOST`                   | Bind address (Default: `0.0.0.0`).                        |
| `PORT`                   | Port number (Default: `8080`).                            |
| `CACHE_SECONDS`          | Overrides the cache duration (seconds) for all endpoints. |
| `FETCH_MULTI_PAGE_STARS` | Positive page limit (e.g. `10` = 1000 repos), `true` = all pages; default `1`.    |
| `EXCLUDE_REPO`           | Comma-separated list of repository names to exclude.      |
| `WHITELIST`              | Comma-separated list of allowed usernames.                |
| `GIST_WHITELIST`         | Comma-separated list of allowed Gist IDs.                 |
| `NODE_ENV`               | Set to `development` to disable HTTP caching.             |

## Usage Examples

```bash
# Get general user stats
curl 'http://127.0.0.1:8080/api?username=h-sumiya&show_icons=true'

# Get top languages used
curl 'http://127.0.0.1:8080/api/top-langs?username=h-sumiya&layout=compact'
```

## GitHub Stats Extended API features

Aligned with successor revision `d130fe21d880fef33060082dffbfaa1a15445b37` (2026-10-22).
Existing card URLs and the standalone binary deployment continue to work.

| Endpoint | Additional query parameters |
| --- | --- |
| `/api`, `/api/top-langs` | `role=OWNER,ORGANIZATION_MEMBER,COLLABORATOR` (case-insensitive; default `OWNER`) |
| All cards | `theme_light`, `theme_dark`, and `title_color`, `icon_color`, `text_color`, `ring_color`, `bg_color`, `border_color`, `prog_bar_bg_color` with `_light` / `_dark` suffixes |
| `/api` | `show=contributions,all_time_contribs`; `contribs_include_own_repos=true` includes owned repositories in both contributed-to stats |
| `/api`, `/api/pin` | `show=prs_authored,prs_commented,prs_reviewed,issues_authored,issues_commented` |
| `/api` | `repo=user/repo,org/repo` and/or `owner=user,org` scope the new REST counts and `include_all_commits`; short repo names use `username` as owner |
| `/api/pin` | `repo=org/repo` permits a repository owner different from the contributor in `username`; `card_width`; `disable_animations` |
| `/api/gist` | `disable_animations`; descriptions wrap by measured text width |
| `/api/top-langs` | `hide_values=true`; `prog_bar_bg_color=0000` for transparent progress tracks |

Themes include `light_github`, `dark_github`, `light_github_repocard`, and `dark_github_repocard`.
A card with light/dark settings uses SVG CSS `prefers-color-scheme` to follow the viewer's setting.

```text
/api?username=h-sumiya&show=contributions,all_time_contribs&theme_light=light_github&theme_dark=dark_github
/api?username=h-sumiya&owner=org&show=prs_authored,issues_authored&hide=stars,commits,prs,issues,contribs&hide_rank=true
/api/pin?username=h-sumiya&repo=org/project&show=prs_authored,prs_reviewed&card_width=500
/api/top-langs?username=h-sumiya&role=OWNER,ORGANIZATION_MEMBER&hide_values=true&prog_bar_bg_color=0000
```

All-time counts require additional GitHub API calls on a cache miss. Repository counts deduplicate across years and split saturated time ranges. If GitHub cannot provide an exact daily repository count or omits contribution data, the service renders an error. REST search totals follow GitHub’s search semantics: incomplete results are retried twice, then returned with a server log warning for compatibility with upstream. They can remain approximate when GitHub times out.
Network failures retry on the same token with 100ms/1s/3s delays; authentication and rate-limit failures rotate tokens. Partial GraphQL errors and non-success HTTP responses are rejected before decoding statistics.

This port provides the card APIs and in-process cache. The successor's hosted configuration wizard, OAuth/private-access login, and precomputation infrastructure are separate services and are not part of this binary. Private data visibility depends on the configured GitHub token and GitHub profile settings.

## TLS and validation

Reqwest 0.13 uses `rustls-platform-verifier` and the operating system trust store. Keep the deployment host's `ca-certificates` package current, and rebuild the binary when updating Rust TLS libraries.
The existing services use standalone binaries, so there is no Docker image to update in this repository. If packaging in a container later, its OS CA package must also be current.

```bash
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo audit
# Live network check: normal HTTPS succeeds; both compromised CA probes fail.
cargo test --locked --test tls_certificates -- --ignored
cargo build --release --locked
```

Upstream license: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
