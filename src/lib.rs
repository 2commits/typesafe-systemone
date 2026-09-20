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
//! use typesafe_systemone::{Client, Question};
//!
//! # async fn run() -> Result<(), typesafe_systemone::Error> {
//! let client = Client::from_env()?; // reads TYPESAFE_API_KEY
//! let response = client
//!     .system_one(
//!         serde_json::json!({ "message": "Help! My payouts have been failing for 3 days." }),
//!         [
//!             ("is_urgent", Question::noul("Does `message` convey urgency?")),
//!             ("department", Question::choice(
//!                 "Which team should handle `message`?",
//!                 [("billing", Some("Payments, invoicing, refunds")), ("technical", None)],
//!             )),
//!         ],
//!     )
//!     .await?;
//!
//! let urgent = response.answers["is_urgent"].as_noul().unwrap();
//! let team = response.answers["department"].as_choice().unwrap();
//! println!("urgent={urgent:.2} team={} confidence={:.2}", team.choice, team.confidence);
//! # Ok(()) }
//! ```
//!
//! This is an unofficial client. The API contract it follows is documented at
//! <https://docs.typesafe.ai/api>.

mod answer;
mod client;
mod error;
mod question;
mod retry;

pub use answer::{Answer, ChoiceAnswer, ModelInfo, NoulAnswer, ScoreAnswer, SystemOneResponse, Usage};
pub use client::{Client, ClientBuilder, DEFAULT_BASE_URL, DEFAULT_MODEL};
pub use error::{Error, Result};
pub use question::{NoulCriteria, Question};
pub use retry::RetryPolicy;
