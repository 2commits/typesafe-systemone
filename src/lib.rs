//! Async Rust client for the [TypeSafe](https://typesafe.ai) System One API.
//!
//! System One models (Jev) evaluate a `state` against typed questions and return
//! calibrated probabilities instead of generated text. Three primitives exist:
//!
//! * [`Question::noul`] — a yes/no judgment, answered with the probability of "yes".
//! * [`Question::choice`] — pick one option from a set, answered with a full distribution.
//! * [`Question::score`] — a position on an ordered rubric, answered with a weighted score.
//!
//! ```no_run
//! use typesafe_systemone::Client;
//!
//! # async fn run() -> Result<(), typesafe_systemone::Error> {
//! let client = Client::from_env()?; // reads TYPESAFE_API_KEY
//! let response = client
//!     .system_one()
//!     .field("message", "Help! My payouts have been failing for 3 days.")
//!     .noul("is_urgent", "Does `message` convey urgency?")
//!     .choice("department", "Which team should handle `message`?", |c| {
//!         c.option("billing", "Payments, invoicing, refunds")
//!             .option("technical", "Bugs, outages, integrations")
//!             .none_of_the_above("No listed team fits")
//!     })
//!     .score("frustration", "How frustrated is the customer?", ["Calm", "Frustrated", "Very angry"])
//!     .send()
//!     .await?;
//!
//! let urgent = response.noul("is_urgent")?;
//! let team = response.choice("department")?;
//! println!("urgent={urgent:.2} team={} confidence={:.2}", team.choice, team.confidence);
//! # Ok(()) }
//! ```
//!
//! `state` is whatever your questions refer to: pass your own `Serialize` struct with
//! [`SystemOneRequest::state`], or assemble an object with [`SystemOneRequest::field`].
//! Callers that already hold a question map use [`Client::evaluate`] directly.
//!
//! This is an unofficial client. The API contract it follows is documented at
//! <https://docs.typesafe.ai/api>.

#![warn(missing_docs)]

mod answer;
mod client;
mod error;
mod question;
mod request;
mod retry;

pub use answer::{Answer, ChoiceAnswer, ModelInfo, NoulAnswer, ScoreAnswer, SystemOneResponse, Usage};
pub use client::{Client, ClientBuilder, DEFAULT_BASE_URL, DEFAULT_MODEL};
pub use error::{Error, Result};
pub use question::{MAX_CHOICE_OPTIONS, NONE_OF_THE_ABOVE, NoulCriteria, Question};
pub use request::{ChoiceBuilder, SystemOneRequest};
pub use retry::RetryPolicy;
