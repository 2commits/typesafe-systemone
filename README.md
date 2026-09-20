# typesafe-systemone

Async Rust client for the [TypeSafe](https://typesafe.ai) System One API. Jev, TypeSafe's
System One model, evaluates a `state` against typed questions and returns calibrated
probabilities rather than generated text.

This is an **unofficial** client. It follows the public API contract at
<https://docs.typesafe.ai/api> and mirrors the shape of TypeSafe's Python and JavaScript SDKs.

```toml
[dependencies]
typesafe-systemone = "0.1"
```

## Usage

```rust
use typesafe_systemone::{Client, Question};

let client = Client::from_env()?; // TYPESAFE_API_KEY

let response = client
    .system_one(
        serde_json::json!({ "message": "Help! My payouts have been failing for 3 days." }),
        [
            ("is_urgent", Question::noul("Does `message` convey urgency?")),
            ("department", Question::choice(
                "Which team should handle `message`?",
                [
                    ("billing", Some("Payments, invoicing, refunds")),
                    ("technical", Some("Bugs, outages, integrations")),
                    ("none_of_the_above", Some("No listed team fits")),
                ],
            )),
            ("frustration", Question::score("How frustrated is the customer?", ["Calm", "Frustrated", "Very angry"])),
        ],
    )
    .await?;

let urgent: f64 = response.answers["is_urgent"].as_noul().unwrap();
let team = response.answers["department"].as_choice().unwrap();
if team.confidence >= 0.9 {
    route(&team.choice);
} else {
    ask_a_human(team.ranked());
}
```

### Primitives

| Constructor | Answer | Use for |
|---|---|---|
| `Question::noul` / `noul_with_criteria` | `noul: f64` probability of yes | "does this condition hold?" |
| `Question::choice` / `choice_plain` | `choice`, `probabilities`, `confidence` | one of up to 255 options |
| `Question::score` | `score`, `legend`, `probabilities`, `confidence` | position on an ordered rubric |

Ask independent questions about the same state in one call. Add a `none_of_the_above`
option to a Choice when the list may not cover every input.

### Configuration

| Builder | Env var | Default |
|---|---|---|
| `.api_key(..)` | `TYPESAFE_API_KEY` | required |
| `.model(..)` | `TYPESAFE_DEFAULT_MODEL` | `jev-latest` |
| `.base_url(..)` | `TYPESAFE_BASE_URL` | `https://api.typesafe.ai` |
| `.timeout(..)` | | 30 s per request |
| `.retry(RetryPolicy)` | | 2 retries, 0.5 s → 5 s backoff, 25 % jitter, honours `retry-after` |
| `.http_client(reqwest::Client)` | | build our own |

Explicit builder values win over environment variables. Empty or whitespace-only env values are ignored.

Pin a versioned model id (`jev-1.13.0`) once you have tuned confidence thresholds; the
`jev-latest` alias moves when a new release ships.

### Errors

`Error` mirrors the official SDKs: `Authentication` (401), `PermissionDenied` (403),
`NotFound` (404), `BadRequest` (400), `UnprocessableEntity` (422), `RateLimit` (429),
`Overloaded` (529), `Server` (other 5xx), `Connection`, `Timeout`, `ResponseValidation`.
408, 429 and 5xx are retried per `RetryPolicy`; other 4xx are not.

### TLS

`rustls` is the default feature. Use `native-tls` instead, or disable default features and
pass your own `reqwest::Client` via `.http_client(..)` to reuse the TLS backend your
application already ships.

## Limits worth knowing

From the [models page](https://docs.typesafe.ai/models): 64k tokens per request for state
plus all questions, 32k for state plus the longest question, 255 options per Choice.
Accuracy degrades as unrelated material in `state` grows, so keep each request's state to
what its questions need.

## License

MIT OR Apache-2.0.
