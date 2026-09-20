use std::collections::HashMap;

use serde::Deserialize;

/// Answer to a [`Question::Noul`](crate::Question::Noul).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct NoulAnswer {
    /// Probability of "yes", from 0 to 1.
    pub noul: f64,
}

/// Answer to a [`Question::Choice`](crate::Question::Choice).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ChoiceAnswer {
    /// The highest-probability option.
    pub choice: String,
    /// Every option mapped to its probability; sums to 1.
    pub probabilities: HashMap<String, f64>,
    /// Certainty derived from the shape of `probabilities`, from 0 to 1.
    pub confidence: f64,
}

impl ChoiceAnswer {
    /// Options ordered by descending probability.
    pub fn ranked(&self) -> Vec<(&str, f64)> {
        let mut ranked: Vec<(&str, f64)> = self.probabilities.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        ranked
    }
}

/// Answer to a [`Question::Score`](crate::Question::Score).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ScoreAnswer {
    /// Probability-weighted position across the levels; may fall between levels.
    pub score: f64,
    /// Level index (as a string key) → level description.
    pub legend: HashMap<String, String>,
    /// Level index (as a string key) → probability; sums to 1.
    pub probabilities: HashMap<String, f64>,
    /// Certainty derived from the shape of `probabilities`, from 0 to 1.
    pub confidence: f64,
}

/// One answer, tagged with the type of the question that produced it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[non_exhaustive]
pub enum Answer {
    Noul(NoulAnswer),
    Choice(ChoiceAnswer),
    Score(ScoreAnswer),
}

impl Answer {
    /// The yes-probability, if this answers a Noul question.
    pub fn as_noul(&self) -> Option<f64> {
        match self {
            Self::Noul(a) => Some(a.noul),
            _ => None,
        }
    }

    /// The choice answer, if this answers a Choice question.
    pub fn as_choice(&self) -> Option<&ChoiceAnswer> {
        match self {
            Self::Choice(a) => Some(a),
            _ => None,
        }
    }

    /// The score answer, if this answers a Score question.
    pub fn as_score(&self) -> Option<&ScoreAnswer> {
        match self {
            Self::Score(a) => Some(a),
            _ => None,
        }
    }
}

/// Token usage for one request. Output tokens are not billed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Response from `POST /v1/systemone`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SystemOneResponse {
    /// The versioned model id that answered, e.g. `jev-1.13.0`.
    pub model: String,
    /// One answer per question, under the same keys.
    pub answers: HashMap<String, Answer>,
    pub usage: Usage,
}

/// One entry from `GET /v1/models`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ModelInfo {
    /// Model id or alias accepted by the `model` field.
    pub name: String,
    pub description: String,
    pub release_date: String,
}

#[derive(Deserialize)]
pub(crate) struct ModelsResponse {
    pub(crate) models: Vec<ModelInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_all_three_answer_types() {
        let body = json!({
            "model": "jev-1.13.0",
            "answers": {
                "is_urgent": {"type": "noul", "noul": 0.92},
                "department": {"type": "choice", "choice": "technical",
                                "probabilities": {"billing": 0.08, "technical": 0.85, "sales": 0.07}, "confidence": 0.82},
                "frustration": {"type": "score", "score": 1.6,
                                 "legend": {"0": "Calm", "1": "Frustrated", "2": "Very angry"},
                                 "probabilities": {"0": 0.05, "1": 0.3, "2": 0.65}, "confidence": 0.78}
            },
            "usage": {"input_tokens": 312, "output_tokens": 48}
        });
        let resp: SystemOneResponse = serde_json::from_value(body).unwrap();
        assert_eq!(resp.answers["is_urgent"].as_noul(), Some(0.92));
        let dept = resp.answers["department"].as_choice().unwrap();
        assert_eq!(dept.choice, "technical");
        assert_eq!(dept.ranked()[0], ("technical", 0.85));
        let score = resp.answers["frustration"].as_score().unwrap();
        assert_eq!(score.legend["2"], "Very angry");
        assert_eq!(resp.usage.input_tokens, 312);
        assert!(resp.answers["is_urgent"].as_choice().is_none());
    }
}
