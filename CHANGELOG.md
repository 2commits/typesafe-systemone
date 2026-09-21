# Changelog

## 0.2.0 — 2026-09-21

**Breaking**
- `Question` and its variants are `#[non_exhaustive]`: construct through `Question::noul` / `choice` / `score` or the request builder, match with `..`.
- `SystemOneRequest::send` now returns `Error::InvalidRequest` for a Choice with a repeated option name or more than `MAX_CHOICE_OPTIONS` (255) options, and validates questions added through `.question(..)` the same way as builder-made ones.

**Added**
- `MAX_CHOICE_OPTIONS`.

## 0.1.1 — 2026-09-21

- `RetryPolicy`: a honoured `retry-after` is now capped at `backoff_max` (previously uncapped; the cap expression was a no-op).
- `ClientBuilder::build` fails with `Error::Config` when the crate has no TLS backend and no `http_client` was supplied, instead of failing on the first request.
- `# Errors` sections on every fallible public function; `missing_docs` enforced; `#[must_use]` on builders and pure getters.
- Internal: removed a redundant clone per call, `const fn` where possible, lint table in `Cargo.toml` (`clippy::all` deny, `pedantic` warn).

## 0.1.0 — 2026-09-20

- Initial release: async client, `system_one()` request builder, typed answers, retry policy.
