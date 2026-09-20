//! Live smoke test. Needs `TYPESAFE_API_KEY`.
//!
//! ```sh
//! TYPESAFE_API_KEY=... cargo run --example smoke
//! ```
use typesafe_systemone::Client;

#[tokio::main]
async fn main() -> Result<(), typesafe_systemone::Error> {
    let client = Client::builder().model("jev-1.13.0").build()?;

    let models = client.models().await?;
    println!(
        "models: {}",
        models.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(", ")
    );

    let response = client
        .system_one()
        .field("raw_value", "Sr. Backend Eng")
        .field("column", "job title")
        .choice(
            "canonical",
            "Which canonical job title does `raw_value` refer to? Match on meaning, not spelling.",
            |c| {
                c.options_plain(["Backend Engineer", "Backend Lead", "Frontend Engineer", "SRE"])
                    .none_of_the_above("A different role")
            },
        )
        .noul("is_senior", "Does `raw_value` indicate a senior-level role?")
        .score(
            "seniority",
            "How senior is `raw_value`?",
            ["Junior", "Mid", "Senior", "Lead"],
        )
        .send()
        .await?;

    println!("model: {}  usage: {:?}", response.model, response.usage);
    let canonical = response.choice("canonical")?;
    println!(
        "canonical: {} (confidence {:.2})",
        canonical.choice, canonical.confidence
    );
    for (opt, p) in canonical.ranked().into_iter().take(3) {
        println!("  {opt:<20} {p:.3}");
    }
    println!("is_senior: {:.2}", response.noul("is_senior")?);
    let seniority = response.score("seniority")?;
    println!(
        "seniority: {:.2} (confidence {:.2})",
        seniority.score, seniority.confidence
    );
    Ok(())
}
