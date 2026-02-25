# github-readme-stats-rs

A Rust port of the original [github-readme-stats](https://github.com/anuraghazra/github-readme-stats). Built with [axum](https://github.com/tokio-rs/axum), it operates as a high-performance single binary without requiring Docker.

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
| `FETCH_MULTI_PAGE_STARS` | Set to `true` to enable pagination for stars in stats.    |
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
