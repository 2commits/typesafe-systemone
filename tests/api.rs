use std::time::Duration;

use serde_json::json;
use typesafe_systemone::{Client, Error, Question, RetryPolicy};
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
        .system_one(
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
        .system_one_with_model("jev-preview", "x", [("q", Question::noul("?"))])
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
        .system_one("x", [("q", Question::noul("?"))])
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
        .system_one("x", [("q", Question::score("?", ["a"]))])
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
        .system_one("x", [("q", Question::noul("?"))])
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
        .system_one("x", [("q", Question::noul("?"))])
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
    let err = c.system_one("x", [("q", Question::noul("?"))]).await.unwrap_err();
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
        .system_one("x", [("q", Question::noul("?"))])
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
