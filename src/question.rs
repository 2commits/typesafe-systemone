use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

/// Conventional name of the abstain option in a Choice. The model always picks *some*
/// option, so give it one that means "nothing here fits".
pub const NONE_OF_THE_ABOVE: &str = "none_of_the_above";

/// Optional descriptions of what a "yes" and a "no" mean for a [`Question::Noul`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct NoulCriteria {
    /// What a yes (value near 1) means.
    #[serde(rename = "true", skip_serializing_if = "Option::is_none")]
    pub yes: Option<String>,
    /// What a no (value near 0) means.
    #[serde(rename = "false", skip_serializing_if = "Option::is_none")]
    pub no: Option<String>,
}

/// A typed question evaluated against the request `state`.
///
/// `instructions` may be a string, object or array. Reference nested state with
/// backticked paths such as `` `rows[3].country` ``.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    /// Yes/no. Answered with the probability of "yes".
    Noul {
        /// The question, referring to the state.
        instructions: Value,
        /// What yes and no mean, when spelled out.
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    /// One option out of a defined set (at most 255). Answered with the chosen option
    /// and the probability of every option.
    Choice {
        /// The question, referring to the state.
        instructions: Value,
        /// Option → optional rubric description.
        criteria: BTreeMap<String, Option<String>>,
    },
    /// A position along an ordered rubric of at least two levels.
    Score {
        /// The question, referring to the state.
        instructions: Value,
        /// Level descriptions, lowest first.
        criteria: Vec<String>,
    },
}

impl Question {
    /// A yes/no question without criteria descriptions.
    pub fn noul(instructions: impl Into<Value>) -> Self {
        Self::Noul {
            instructions: instructions.into(),
            criteria: None,
        }
    }

    /// A yes/no question with descriptions of what yes and no mean.
    pub fn noul_with_criteria(instructions: impl Into<Value>, yes: impl Into<String>, no: impl Into<String>) -> Self {
        Self::Noul {
            instructions: instructions.into(),
            criteria: Some(NoulCriteria {
                yes: Some(yes.into()),
                no: Some(no.into()),
            }),
        }
    }

    /// Pick one of `options`. Each option is `(name, optional description)`.
    pub fn choice<K, V>(instructions: impl Into<Value>, options: impl IntoIterator<Item = (K, Option<V>)>) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        Self::Choice {
            instructions: instructions.into(),
            criteria: options
                .into_iter()
                .map(|(k, v)| (k.into(), v.map(Into::into)))
                .collect(),
        }
    }

    /// Pick one of `options`, none of which carries a description.
    pub fn choice_plain<K: Into<String>>(instructions: impl Into<Value>, options: impl IntoIterator<Item = K>) -> Self {
        Self::choice(instructions, options.into_iter().map(|k| (k, None::<String>)))
    }

    /// Rate along `levels`, ordered from lowest to highest.
    pub fn score<L: Into<String>>(instructions: impl Into<Value>, levels: impl IntoIterator<Item = L>) -> Self {
        Self::Score {
            instructions: instructions.into(),
            criteria: levels.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn noul_serializes_without_criteria_when_absent() {
        let q = Question::noul("Does this convey urgency?");
        assert_eq!(
            serde_json::to_value(&q).unwrap(),
            json!({"type": "noul", "instructions": "Does this convey urgency?"})
        );
    }

    #[test]
    fn noul_criteria_use_true_false_keys() {
        let q = Question::noul_with_criteria("Urgent?", "Explicitly time-sensitive", "No urgency expressed");
        assert_eq!(
            serde_json::to_value(&q).unwrap(),
            json!({
                "type": "noul",
                "instructions": "Urgent?",
                "criteria": {"true": "Explicitly time-sensitive", "false": "No urgency expressed"}
            })
        );
    }

    #[test]
    fn choice_serializes_null_for_undescribed_options() {
        let q = Question::choice("Which team?", [("billing", Some("Payments")), ("sales", None)]);
        assert_eq!(
            serde_json::to_value(&q).unwrap(),
            json!({"type": "choice", "instructions": "Which team?", "criteria": {"billing": "Payments", "sales": null}})
        );
    }

    #[test]
    fn score_serializes_levels_in_order() {
        let q = Question::score(json!({"rate": "frustration"}), ["Calm", "Frustrated", "Very angry"]);
        assert_eq!(
            serde_json::to_value(&q).unwrap(),
            json!({"type": "score", "instructions": {"rate": "frustration"}, "criteria": ["Calm", "Frustrated", "Very angry"]})
        );
    }
}
