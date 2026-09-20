use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::answer::{ModelInfo, ModelsResponse, SystemOneResponse};
use crate::error::{Error, Result};
use crate::question::Question;
use crate::request::SystemOneRequest;
use crate::retry::RetryPolicy;

/// Production API root.
pub const DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";
/// Model used when none is configured: the latest stable Jev release.
pub const DEFAULT_MODEL: &str = "jev-latest";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_ERROR_BODY: usize = 2048;

const ENV_API_KEY: &str = "TYPESAFE_API_KEY";
const ENV_MODEL: &str = "TYPESAFE_DEFAULT_MODEL";
const ENV_BASE_URL: &str = "TYPESAFE_BASE_URL";

struct Inner {
    http: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
    retry: RetryPolicy,
    timeout: Duration,
}

/// Client for the TypeSafe System One API. Cheap to clone; shares one HTTP pool.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.inner.base_url)
            .field("model", &self.inner.model)
            .field("retry", &self.inner.retry)
            .field("timeout", &self.inner.timeout)
            .finish_non_exhaustive()
    }
}

/// Builder for [`Client`]. Explicit values win over environment variables.
#[derive(Default)]
pub struct ClientBuilder {
    api_key: Option<String>,
    base_url: Option<String>,
    model: Option<String>,
    retry: Option<RetryPolicy>,
    timeout: Option<Duration>,
    http: Option<reqwest::Client>,
}

impl fmt::Debug for ClientBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("retry", &self.retry)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl ClientBuilder {
    /// API key. Falls back to `TYPESAFE_API_KEY`.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// API root without trailing slash. Falls back to `TYPESAFE_BASE_URL`, then
    /// [`DEFAULT_BASE_URL`].
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Default model for [`Client::system_one`]. Falls back to `TYPESAFE_DEFAULT_MODEL`,
    /// then [`DEFAULT_MODEL`]. Pin a versioned id (e.g. `jev-1.13.0`) once thresholds are tuned.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Retry policy. Defaults to [`RetryPolicy::default`].
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = Some(retry);
        self
    }

    /// Per-request timeout. Defaults to 30 seconds.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Reuse an existing `reqwest::Client` (TLS backend, proxies, pools). When set, this
    /// crate's own TLS features are irrelevant.
    pub fn http_client(mut self, http: reqwest::Client) -> Self {
        self.http = Some(http);
        self
    }

    /// Build the client. Fails when no API key is available.
    pub fn build(self) -> Result<Client> {
        let api_key = self
            .api_key
            .filter(|k| !k.trim().is_empty())
            .or_else(|| env_non_empty(ENV_API_KEY))
            .ok_or_else(|| Error::Config(format!("no API key: pass ClientBuilder::api_key or set {ENV_API_KEY}")))?;
        let base_url = self
            .base_url
            .or_else(|| env_non_empty(ENV_BASE_URL))
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned())
            .trim_end_matches('/')
            .to_owned();
        let model = self
            .model
            .or_else(|| env_non_empty(ENV_MODEL))
            .unwrap_or_else(|| DEFAULT_MODEL.to_owned());
        let http = match self.http {
            Some(h) => h,
            None => reqwest::Client::builder()
                .build()
                .map_err(|e| Error::Config(format!("failed to build HTTP client: {e}")))?,
        };
        Ok(Client {
            inner: Arc::new(Inner {
                http,
                api_key,
                base_url,
                model,
                retry: self.retry.unwrap_or_default(),
                timeout: self.timeout.unwrap_or(DEFAULT_TIMEOUT),
            }),
        })
    }
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

#[derive(Serialize)]
struct SystemOneBody<'a> {
    state: &'a Value,
    model: &'a str,
    questions: &'a BTreeMap<String, Question>,
}

impl Client {
    /// Start configuring a client.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// Client configured entirely from `TYPESAFE_API_KEY`, `TYPESAFE_DEFAULT_MODEL` and
    /// `TYPESAFE_BASE_URL`.
    pub fn from_env() -> Result<Self> {
        Self::builder().build()
    }

    /// The model used when [`Client::system_one`] is called.
    pub fn default_model(&self) -> &str {
        &self.inner.model
    }

    /// Start building a `POST /v1/systemone` call: state, questions, then `.send()`.
    pub fn system_one(&self) -> SystemOneRequest<'_> {
        SystemOneRequest::new(self)
    }

    /// Evaluate `state` against an existing map of `questions` with the client's default
    /// model. The builder form is [`Client::system_one`].
    ///
    /// Questions are keyed by the ids you choose; answers come back under the same ids.
    /// All questions see the same state and are evaluated independently.
    pub async fn evaluate<K>(
        &self,
        state: impl Serialize,
        questions: impl IntoIterator<Item = (K, Question)>,
    ) -> Result<SystemOneResponse>
    where
        K: Into<String>,
    {
        let model = self.inner.model.clone();
        self.evaluate_with_model(&model, state, questions).await
    }

    /// [`Client::evaluate`] with an explicit `model`.
    pub async fn evaluate_with_model<K>(
        &self,
        model: &str,
        state: impl Serialize,
        questions: impl IntoIterator<Item = (K, Question)>,
    ) -> Result<SystemOneResponse>
    where
        K: Into<String>,
    {
        let state = serde_json::to_value(state).map_err(Error::RequestSerialization)?;
        let questions: BTreeMap<String, Question> = questions.into_iter().map(|(k, q)| (k.into(), q)).collect();
        let body = serde_json::to_vec(&SystemOneBody {
            state: &state,
            model,
            questions: &questions,
        })
        .map_err(Error::RequestSerialization)?;
        let bytes = self.send(reqwest::Method::POST, "/v1/systemone", Some(body)).await?;
        serde_json::from_slice(&bytes).map_err(Error::ResponseValidation)
    }

    /// List the model names and aliases this account may send in the `model` field.
    pub async fn models(&self) -> Result<Vec<ModelInfo>> {
        let bytes = self.send(reqwest::Method::GET, "/v1/models", None).await?;
        let parsed: ModelsResponse = serde_json::from_slice(&bytes).map_err(Error::ResponseValidation)?;
        Ok(parsed.models)
    }

    async fn send(&self, method: reqwest::Method, path: &str, body: Option<Vec<u8>>) -> Result<Vec<u8>> {
        let url = format!("{}{}", self.inner.base_url, path);
        let mut retry = 0u32;
        loop {
            let mut req = self
                .inner
                .http
                .request(method.clone(), &url)
                .bearer_auth(&self.inner.api_key)
                .timeout(self.inner.timeout);
            if let Some(b) = &body {
                req = req
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(b.clone());
            }

            let outcome = match req.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if (200..300).contains(&status) {
                        return resp.bytes().await.map(|b| b.to_vec()).map_err(Error::from_transport);
                    }
                    let retry_after = parse_retry_after(resp.headers());
                    let message = read_error_body(resp).await;
                    let err = Error::from_status(status, retry_after, message);
                    if RetryPolicy::is_retryable_status(status) {
                        Err((err, retry_after))
                    } else {
                        return Err(err);
                    }
                }
                Err(e) => Err((Error::from_transport(e), None)),
            };

            let (err, retry_after) = match outcome {
                Ok(never) => never,
                Err(pair) => pair,
            };
            if retry >= self.inner.retry.max_retries {
                return Err(err);
            }
            retry += 1;
            tokio::time::sleep(self.inner.retry.delay(retry, retry_after)).await;
        }
    }
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map(Duration::from_secs_f64)
}

async fn read_error_body(resp: reqwest::Response) -> String {
    let text = resp.text().await.unwrap_or_default();
    let message = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| {
            ["detail", "message", "error"].into_iter().find_map(|k| {
                v.get(k)
                    .map(|m| m.as_str().map(str::to_owned).unwrap_or_else(|| m.to_string()))
            })
        })
        .unwrap_or(text);
    if message.len() > MAX_ERROR_BODY {
        let mut end = MAX_ERROR_BODY;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &message[..end])
    } else {
        message
    }
}
