use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::answer::SystemOneResponse;
use crate::client::Client;
use crate::error::{Error, Result};
use crate::question::{NONE_OF_THE_ABOVE, Question};

enum State {
    Empty,
    Whole(Value),
    Fields(Map<String, Value>),
}

/// A `POST /v1/systemone` call under construction. Created by [`Client::system_one`].
///
/// Set the state once with [`state`](Self::state) (any `Serialize`, typically your own
/// struct) or build it field by field with [`field`](Self::field), add questions, then
/// [`send`](Self::send). Errors in the inputs (unserializable value, fields added to a
/// non-object state, no state, no questions) surface from `send`, so the chain stays clean.
#[must_use = "a request does nothing until `.send().await`"]
pub struct SystemOneRequest<'a> {
    client: &'a Client,
    model: Option<String>,
    state: State,
    questions: BTreeMap<String, Question>,
    error: Option<Error>,
}

impl<'a> SystemOneRequest<'a> {
    pub(crate) fn new(client: &'a Client) -> Self {
        Self {
            client,
            model: None,
            state: State::Empty,
            questions: BTreeMap::new(),
            error: None,
        }
    }

    /// Model for this call only. Defaults to the client's model.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// The whole state: a string, or any `Serialize` value (your own struct, a `Vec`, a
    /// `serde_json::Value`). Replaces anything set before.
    pub fn state(mut self, state: impl Serialize) -> Self {
        match serde_json::to_value(state) {
            Ok(v) => self.state = State::Whole(v),
            Err(e) => self.fail(Error::RequestSerialization(e)),
        }
        self
    }

    /// One named field of an object state. Call it once per field; a later
    /// [`state`](Self::state) replaces them all. Fails at `send` if the state was already
    /// set to something that is not an object.
    pub fn field(mut self, name: impl Into<String>, value: impl Serialize) -> Self {
        let value = match serde_json::to_value(value) {
            Ok(v) => v,
            Err(e) => {
                self.fail(Error::RequestSerialization(e));
                return self;
            }
        };
        let name = name.into();
        self.state = match std::mem::replace(&mut self.state, State::Empty) {
            State::Empty => State::Fields(Map::from_iter([(name, value)])),
            State::Fields(mut map) | State::Whole(Value::Object(mut map)) => {
                map.insert(name, value);
                State::Fields(map)
            }
            other @ State::Whole(_) => {
                self.fail(Error::InvalidRequest(format!(
                    "cannot add field `{name}`: state is not an object"
                )));
                other
            }
        };
        self
    }

    /// Any question under `id`. Answers come back under the same id.
    pub fn question(mut self, id: impl Into<String>, question: Question) -> Self {
        let id = id.into();
        if self.questions.insert(id.clone(), question).is_some() {
            self.fail(Error::InvalidRequest(format!("duplicate question id `{id}`")));
        }
        self
    }

    /// Yes/no question.
    pub fn noul(self, id: impl Into<String>, instructions: impl Into<Value>) -> Self {
        self.question(id, Question::noul(instructions))
    }

    /// Yes/no question with descriptions of what yes and no mean.
    pub fn noul_with_criteria(
        self,
        id: impl Into<String>,
        instructions: impl Into<Value>,
        yes: impl Into<String>,
        no: impl Into<String>,
    ) -> Self {
        self.question(id, Question::noul_with_criteria(instructions, yes, no))
    }

    /// Pick one option; build the option set in the closure. Repeating an option name is an
    /// error at `send`, as is an empty set or more than [`MAX_CHOICE_OPTIONS`](crate::MAX_CHOICE_OPTIONS) options.
    pub fn choice(
        mut self,
        id: impl Into<String>,
        instructions: impl Into<Value>,
        options: impl FnOnce(ChoiceBuilder) -> ChoiceBuilder,
    ) -> Self {
        let id = id.into();
        let built = options(ChoiceBuilder::default());
        if let Some(dup) = built.duplicates.first() {
            self.fail(Error::InvalidRequest(format!("choice `{id}` repeats option `{dup}`")));
            return self;
        }
        self.question(id, Question::choice(instructions, built.options))
    }

    /// Rate along `levels`, lowest first. Fewer than two levels is an error at `send`.
    pub fn score<L: Into<String>>(
        self,
        id: impl Into<String>,
        instructions: impl Into<Value>,
        levels: impl IntoIterator<Item = L>,
    ) -> Self {
        self.question(id, Question::score(instructions, levels))
    }

    /// Send the request.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidRequest`] for a request that is not sendable as built: no state, no
    /// questions, a Choice without options or with more than [`MAX_CHOICE_OPTIONS`](crate::MAX_CHOICE_OPTIONS), a
    /// repeated option name, a Score with fewer than two levels, a duplicate question id,
    /// or a field added to a non-object state. [`Error::RequestSerialization`]
    /// if a `state`/`field` value failed to serialise. Otherwise as [`Client::evaluate`].
    pub async fn send(self) -> Result<SystemOneResponse> {
        if let Some(e) = self.error {
            return Err(e);
        }
        let state = match self.state {
            State::Empty => {
                return Err(Error::InvalidRequest(
                    "no state: call `.state(..)` or `.field(..)`".into(),
                ));
            }
            State::Whole(v) => v,
            State::Fields(map) => Value::Object(map),
        };
        if self.questions.is_empty() {
            return Err(Error::InvalidRequest("no questions".into()));
        }
        if let Some((id, problem)) = self
            .questions
            .iter()
            .find_map(|(id, q)| q.structural_problem().map(|p| (id, p)))
        {
            return Err(Error::InvalidRequest(format!("question `{id}` {problem}")));
        }
        match self.model {
            Some(model) => self.client.evaluate_with_model(&model, state, self.questions).await,
            None => self.client.evaluate(state, self.questions).await,
        }
    }

    fn fail(&mut self, e: Error) {
        if self.error.is_none() {
            self.error = Some(e);
        }
    }
}

/// Option set of a Choice, built inside [`SystemOneRequest::choice`].
#[derive(Default)]
#[must_use = "return the builder from the `choice` closure"]
pub struct ChoiceBuilder {
    options: BTreeMap<String, Option<String>>,
    duplicates: Vec<String>,
}

impl ChoiceBuilder {
    fn insert(&mut self, name: String, description: Option<String>) {
        if self.options.insert(name.clone(), description).is_some() {
            self.duplicates.push(name);
        }
    }

    /// An option with a rubric description.
    pub fn option(mut self, name: impl Into<String>, description: impl Into<String>) -> Self {
        self.insert(name.into(), Some(description.into()));
        self
    }

    /// An option that needs no description.
    pub fn option_plain(mut self, name: impl Into<String>) -> Self {
        self.insert(name.into(), None);
        self
    }

    /// Many undescribed options at once.
    pub fn options_plain<K: Into<String>>(mut self, names: impl IntoIterator<Item = K>) -> Self {
        for name in names {
            self.insert(name.into(), None);
        }
        self
    }

    /// Many described options at once.
    pub fn options<K, V>(mut self, pairs: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        for (name, description) in pairs {
            self.insert(name.into(), Some(description.into()));
        }
        self
    }

    /// The abstain option (`none_of_the_above`). A Choice always picks *something*; this is
    /// how the model says nothing in the list fits.
    pub fn none_of_the_above(self, description: impl Into<String>) -> Self {
        self.option(NONE_OF_THE_ABOVE, description)
    }
}
