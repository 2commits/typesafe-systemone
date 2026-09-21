//! Integration tests against a wiremock stand-in for `api.typesafe.ai`.
#![allow(
    clippy::float_cmp,
    reason = "probabilities are parsed verbatim from JSON literals, exact comparison is intended"
)]

use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use typesafe_systemone::{Client, Error, MAX_CHOICE_OPTIONS, Question, RetryPolicy};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> Client {
    Client::builder()
        .api_key("test-key")
        .base_url(server.uri())
        .model("jev-1.13.0")
        .retry(RetryPolicy {
            backoff_initial: Duration::from_millis(1),
            backoff_max: Duration::from_millis(5),
            ..RetryPolicy::default()
        })
        .build()
        .unwrap()
}

fn golden_response() -> serde_json::Value {
    json!({
        "model": "jev-1.13.0",
        "answers": {
            "is_urgent": {"type": "noul", "noul": 0.92},
            "department": {"type": "choice", "choice": "technical",
                            "probabilities": {"billing": 0.08, "technical": 0.85, "sales": 0.07}, "confidence": 0.82}
        },
        "usage": {"input_tokens": 312, "output_tokens": 48}
    })
}

#[tokio::test]
async fn system_one_sends_documented_body_and_parses_answers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .and(header("authorization", "Bearer test-key"))
        .and(header("content-type", "application/json"))
        .and(body_json(json!({
            "state": {"message": "Help! My payouts have been failing for 3 days."},
            "model": "jev-1.13.0",
            "questions": {
                "is_urgent": {"type": "noul", "instructions": "Does `message` convey urgency?"},
                "department": {"type": "choice", "instructions": "Which team?",
                                "criteria": {"billing": "Payments", "sales": null, "technical": null}}
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(golden_response()))
        .expect(1)
        .mount(&server)
        .await;

    let resp = client(&server)
        .evaluate(
            json!({"message": "Help! My payouts have been failing for 3 days."}),
            [
                ("is_urgent", Question::noul("Does `message` convey urgency?")),
                (
                    "department",
                    Question::choice(
                        "Which team?",
                        [("billing", Some("Payments")), ("sales", None), ("technical", None)],
                    ),
                ),
            ],
        )
        .await
        .unwrap();

    assert_eq!(resp.model, "jev-1.13.0");
    assert_eq!(resp.answers["is_urgent"].as_noul(), Some(0.92));
    assert_eq!(resp.answers["department"].as_choice().unwrap().choice, "technical");
    assert_eq!(resp.usage.output_tokens, 48);
}

#[tokio::test]
async fn explicit_model_overrides_default() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .and(body_json(json!({"state": "x", "model": "jev-preview",
                              "questions": {"q": {"type": "noul", "instructions": "?"}}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-preview", "answers": {"q": {"type": "noul", "noul": 0.5}},
            "usage": {"input_tokens": 1, "output_tokens": 1}})))
        .expect(1)
        .mount(&server)
        .await;
    let resp = client(&server)
        .evaluate_with_model("jev-preview", "x", [("q", Question::noul("?"))])
        .await
        .unwrap();
    assert_eq!(resp.model, "jev-preview");
}

#[tokio::test]
async fn unauthorized_maps_to_authentication_and_is_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"detail": "Invalid API key"})))
        .expect(1)
        .mount(&server)
        .await;
    let err = client(&server)
        .evaluate("x", [("q", Question::noul("?"))])
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Authentication { ref message } if message == "Invalid API key"),
        "{err:?}"
    );
    assert_eq!(err.status(), Some(401));
}

#[tokio::test]
async fn unprocessable_entity_is_not_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(422).set_body_string("criteria must have at least two levels"))
        .expect(1)
        .mount(&server)
        .await;
    let err = client(&server)
        .evaluate("x", [("q", Question::score("?", ["a"]))])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::UnprocessableEntity { .. }), "{err:?}");
}

#[tokio::test]
async fn rate_limit_is_retried_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .set_body_string("slow down"),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(golden_response()))
        .expect(1)
        .mount(&server)
        .await;
    let resp = client(&server)
        .evaluate("x", [("q", Question::noul("?"))])
        .await
        .unwrap();
    assert_eq!(resp.model, "jev-1.13.0");
}

#[tokio::test]
async fn overloaded_exhausts_retries_and_surfaces_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(529)
                .insert_header("retry-after", "0")
                .set_body_string("overloaded"),
        )
        .expect(3) // 1 attempt + 2 retries
        .mount(&server)
        .await;
    let err = client(&server)
        .evaluate("x", [("q", Question::noul("?"))])
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Overloaded { retry_after: Some(d), .. } if d.is_zero()),
        "{err:?}"
    );
}

#[tokio::test]
async fn retry_none_makes_one_attempt() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).set_body_string("down"))
        .expect(1)
        .mount(&server)
        .await;
    let c = Client::builder()
        .api_key("k")
        .base_url(server.uri())
        .retry(RetryPolicy::none())
        .build()
        .unwrap();
    let err = c.evaluate("x", [("q", Question::noul("?"))]).await.unwrap_err();
    assert!(matches!(err, Error::Server { status: 503, .. }), "{err:?}");
}

#[tokio::test]
async fn malformed_success_body_is_response_validation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"model": "jev", "answers": {"q": {"type": "banana"}}})),
        )
        .mount(&server)
        .await;
    let err = client(&server)
        .evaluate("x", [("q", Question::noul("?"))])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::ResponseValidation(_)), "{err:?}");
}

#[tokio::test]
async fn models_lists_entries() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models": [
            {"name": "jev-latest", "description": "Latest stable", "release_date": "2026-09-01"}
        ]})))
        .mount(&server)
        .await;
    let models = client(&server).models().await.unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].name, "jev-latest");
}

#[tokio::test]
async fn trailing_slash_in_base_url_is_tolerated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"models": []})))
        .expect(1)
        .mount(&server)
        .await;
    let c = Client::builder()
        .api_key("k")
        .base_url(format!("{}/", server.uri()))
        .build()
        .unwrap();
    assert!(c.models().await.unwrap().is_empty());
}

#[test]
fn builder_requires_api_key() {
    // Safety: tests in this binary that touch env run serially by name; none set TYPESAFE_API_KEY.
    unsafe { std::env::remove_var("TYPESAFE_API_KEY") };
    let err = Client::builder().build().unwrap_err();
    assert!(matches!(err, Error::Config(_)), "{err:?}");
    let err = Client::builder().api_key("   ").build().unwrap_err();
    assert!(matches!(err, Error::Config(_)), "{err:?}");
}

#[test]
fn builder_defaults() {
    let c = Client::builder().api_key("k").build().unwrap();
    assert_eq!(c.default_model(), typesafe_systemone::DEFAULT_MODEL);
    let dbg = format!("{c:?}");
    assert!(
        !dbg.contains('k') || !dbg.contains("api_key"),
        "debug output must not leak the key: {dbg}"
    );
}

#[tokio::test]
async fn builder_sends_documented_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .and(body_json(json!({
            "state": {"message": "Help! My payouts have been failing for 3 days.", "tier": "gold"},
            "model": "jev-1.13.0",
            "questions": {
                "is_urgent": {"type": "noul", "instructions": "Does `message` convey urgency?"},
                "department": {"type": "choice", "instructions": "Which team?",
                                "criteria": {"billing": "Payments", "technical": null, "none_of_the_above": "No listed team fits"}},
                "frustration": {"type": "score", "instructions": "How frustrated?", "criteria": ["Calm", "Angry"]}
            }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(golden_response()))
        .expect(1)
        .mount(&server)
        .await;

    let resp = client(&server)
        .system_one()
        .field("message", "Help! My payouts have been failing for 3 days.")
        .field("tier", "gold")
        .noul("is_urgent", "Does `message` convey urgency?")
        .choice("department", "Which team?", |c| {
            c.option("billing", "Payments")
                .option_plain("technical")
                .none_of_the_above("No listed team fits")
        })
        .score("frustration", "How frustrated?", ["Calm", "Angry"])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.noul("is_urgent").unwrap(), 0.92);
    assert_eq!(resp.choice("department").unwrap().choice, "technical");
}

#[tokio::test]
async fn builder_accepts_a_typed_state_and_per_call_model() {
    #[derive(Serialize)]
    struct Ticket<'a> {
        subject: &'a str,
        messages: Vec<&'a str>,
    }
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_json(json!({
            "state": {"subject": "Duplicate charge", "messages": ["I was charged twice"]},
            "model": "jev-preview",
            "questions": {"refund": {"type": "noul", "instructions": "Refund requested?"}}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-preview", "answers": {"refund": {"type": "noul", "noul": 0.9}},
            "usage": {"input_tokens": 1, "output_tokens": 1}})))
        .expect(1)
        .mount(&server)
        .await;
    let resp = client(&server)
        .system_one()
        .model("jev-preview")
        .state(Ticket {
            subject: "Duplicate charge",
            messages: vec!["I was charged twice"],
        })
        .noul("refund", "Refund requested?")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.noul("refund").unwrap(), 0.9);
}

/// A server that must never be reached: these requests fail before sending.
async fn unreachable_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn send_rejects_missing_state() {
    let server = unreachable_server().await;
    let err = client(&server).system_one().noul("q", "?").send().await.unwrap_err();
    assert!(
        matches!(err, Error::InvalidRequest(ref m) if m.contains("no state")),
        "{err:?}"
    );
}

#[tokio::test]
async fn send_rejects_missing_questions() {
    let server = unreachable_server().await;
    let err = client(&server).system_one().field("a", 1).send().await.unwrap_err();
    assert!(
        matches!(err, Error::InvalidRequest(ref m) if m.contains("no questions")),
        "{err:?}"
    );
}

#[tokio::test]
async fn send_rejects_field_on_non_object_state() {
    let server = unreachable_server().await;
    let err = client(&server)
        .system_one()
        .state("plain text")
        .field("a", 1)
        .noul("q", "?")
        .send()
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::InvalidRequest(ref m) if m.contains("not an object")),
        "{err:?}"
    );
}

#[tokio::test]
async fn send_rejects_duplicate_question_id() {
    let server = unreachable_server().await;
    let err = client(&server)
        .system_one()
        .field("a", 1)
        .noul("q", "?")
        .noul("q", "again")
        .send()
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::InvalidRequest(ref m) if m.contains("duplicate")),
        "{err:?}"
    );
}

#[tokio::test]
async fn send_rejects_choice_without_options() {
    let server = unreachable_server().await;
    let err = client(&server)
        .system_one()
        .field("a", 1)
        .choice("c", "?", |c| c)
        .send()
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::InvalidRequest(ref m) if m.contains("no options")),
        "{err:?}"
    );
}

#[tokio::test]
async fn send_rejects_repeated_choice_option() {
    let server = unreachable_server().await;
    let err = client(&server)
        .system_one()
        .field("a", 1)
        .choice("c", "?", |c| c.option_plain("x").option("x", "again"))
        .send()
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::InvalidRequest(ref m) if m.contains("repeats option `x`")),
        "{err:?}"
    );
}

#[tokio::test]
async fn send_rejects_choice_over_the_option_limit_from_a_prebuilt_question() {
    let server = unreachable_server().await;
    let too_many = Question::choice_plain("?", (0..=MAX_CHOICE_OPTIONS).map(|i| format!("o{i}")));
    let err = client(&server)
        .system_one()
        .field("a", 1)
        .question("c", too_many)
        .send()
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::InvalidRequest(ref m) if m.contains("at most 255")),
        "{err:?}"
    );
}

#[tokio::test]
async fn send_rejects_score_with_one_level() {
    let server = unreachable_server().await;
    let err = client(&server)
        .system_one()
        .field("a", 1)
        .score("s", "?", ["one"])
        .send()
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::InvalidRequest(ref m) if m.contains("two levels")),
        "{err:?}"
    );
}

#[tokio::test]
async fn builder_merges_fields_into_an_object_state() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_json(json!({"state": {"a": 1, "b": 2}, "model": "jev-1.13.0",
                              "questions": {"q": {"type": "noul", "instructions": "?"}}})))
        .respond_with(ResponseTemplate::new(200).set_body_json(golden_response()))
        .expect(1)
        .mount(&server)
        .await;
    client(&server)
        .system_one()
        .state(json!({"a": 1}))
        .field("b", 2)
        .noul("q", "?")
        .send()
        .await
        .unwrap();
}
