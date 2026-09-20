//! Live smoke test. Needs `TYPESAFE_API_KEY`.
//!
//! ```sh
//! TYPESAFE_API_KEY=... cargo run --example smoke
//! ```
use serde_json::json;
use typesafe_systemone::{Client, Question};

#[tokio::main]
async fn main() -> Result<(), typesafe_systemone::Error> {
    let client = Client::builder().model("jev-1.13.0").build()?;

    let models = client.models().await?;
    println!(
        "models: {}",
        models.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", ")
    );

    let response = client
        .system_one(
            json!({"raw_value": "Sr. Backend Eng", "column": "job title"}),
            [
                (
                    "canonical",
                    Question::choice_plain(
                        "Which canonical job title does `raw_value` refer to? Match on meaning, not spelling.",
                        [
                            "Backend Engineer",
                            "Backend Lead",
                            "Frontend Engineer",
                            "SRE",
                            "none_of_the_above",
                        ],
                    ),
                ),
                (
                    "is_senior",
                    Question::noul("Does `raw_value` indicate a senior-level role?"),
                ),
                (
                    "seniority",
                    Question::score("How senior is `raw_value`?", ["Junior", "Mid", "Senior", "Lead"]),
                ),
            ],
        )
        .await?;

    println!("model: {}  usage: {:?}", response.model, response.usage);
    let canonical = response.answers["canonical"].as_choice().unwrap();
    println!(
        "canonical: {} (confidence {:.2})",
        canonical.choice, canonical.confidence
    );
    for (opt, p) in canonical.ranked().into_iter().take(3) {
        println!("  {opt:<20} {p:.3}");
    }
    println!("is_senior: {:.2}", response.answers["is_senior"].as_noul().unwrap());
    let seniority = response.answers["seniority"].as_score().unwrap();
    println!(
        "seniority: {:.2} (confidence {:.2})",
        seniority.score, seniority.confidence
    );
    Ok(())
}
