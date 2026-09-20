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
use typesafe_systemone::Client;

let client = Client::from_env()?; // TYPESAFE_API_KEY

let response = client
    .system_one()
    .field("message", "Help! My payouts have been failing for 3 days.")
    .noul("is_urgent", "Does `message` convey urgency?")
    .choice("department", "Which team should handle `message`?", |c| {
        c.option("billing", "Payments, invoicing, refunds")
            .option("technical", "Bugs, outages, integrations")
            .none_of_the_above("No listed team fits")
    })
    .score("frustration", "How frustrated is the customer?", ["Calm", "Frustrated", "Very angry"])
    .send()
    .await?;

let urgent: f64 = response.noul("is_urgent")?;
let team = response.choice("department")?;
if team.confidence >= 0.9 {
    route(&team.choice);
} else {
    ask_a_human(team.ranked());
}
```

`state` is whatever the questions refer to. Build it with `.field(name, value)` calls, or
pass one `Serialize` value with `.state(my_struct)`; `.model(..)` overrides the client's
model for one call. Input mistakes (no state, no questions, a Choice without options, a
duplicate id) come back from `.send()` as `Error::InvalidRequest`, so the chain stays clean.

Already holding a question map? `client.evaluate(state, questions)` takes `(id, Question)`
pairs directly.

### Primitives

| Builder | Read the answer | Use for |
|---|---|---|
| `.noul(id, q)` / `.noul_with_criteria(..)` | `response.noul(id)?` (probability of yes) | "does this condition hold?" |
| `.choice(id, q, closure)` with `.option(..)`, `.options_plain(..)`, `.none_of_the_above(..)` | `response.choice(id)?` (`choice`, `probabilities`, `confidence`, `ranked()`) | one of up to 255 options |
| `.score(id, q, levels)` | `response.score(id)?` (`score`, `legend`, `probabilities`, `confidence`) | position on an ordered rubric |

`Question::noul` / `choice` / `score` constructors exist too, for `.question(id, q)` and `client.evaluate(..)`.

Ask independent questions about the same state in one call. Add `.none_of_the_above(..)`
to a Choice when the list may not cover every input; a Choice always picks something.

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
