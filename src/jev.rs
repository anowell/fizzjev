//! A minimal typed client for the TypeSafe System One endpoint.
//! See https://docs.typesafe.ai/api

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const MAX_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    Noul {
        instructions: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        criteria: Option<NoulCriteria>,
    },
    Choice {
        instructions: Value,
        criteria: Map<String, Value>,
    },
    Score {
        instructions: Value,
        criteria: Vec<Value>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    pub yes: Value,
    #[serde(rename = "false")]
    pub no: Value,
}

impl Question {
    pub fn noul(instructions: impl Into<Value>) -> Self {
        Question::Noul {
            instructions: instructions.into(),
            criteria: None,
        }
    }

    pub fn noul_with(
        instructions: impl Into<Value>,
        yes: impl Into<Value>,
        no: impl Into<Value>,
    ) -> Self {
        Question::Noul {
            instructions: instructions.into(),
            criteria: Some(NoulCriteria {
                yes: yes.into(),
                no: no.into(),
            }),
        }
    }

    pub fn choice<'a>(
        instructions: impl Into<Value>,
        options: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Self {
        let criteria = options
            .into_iter()
            .map(|(k, v)| (k.to_string(), Value::from(v)))
            .collect();
        Question::Choice {
            instructions: instructions.into(),
            criteria,
        }
    }

    pub fn score<'a>(
        instructions: impl Into<Value>,
        levels: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        Question::Score {
            instructions: instructions.into(),
            criteria: levels.into_iter().map(Value::from).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: f64,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

impl Usage {
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub answers: BTreeMap<String, Answer>,
    #[serde(default)]
    pub usage: Usage,
}

#[derive(Serialize)]
struct Request<'a> {
    state: &'a Value,
    model: &'a str,
    questions: &'a BTreeMap<String, Question>,
}

pub struct Client {
    http: reqwest::Client,
    api_key: String,
    model: String,
}

impl Client {
    pub fn from_env(model: impl Into<String>) -> Result<Self> {
        let api_key = std::env::var("TYPESAFE_API_KEY")
            .context("TYPESAFE_API_KEY is not set (run via `just`, which loads the dotenv file)")?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            api_key,
            model: model.into(),
        })
    }

    pub async fn ask(
        &self,
        state: &Value,
        questions: &BTreeMap<String, Question>,
    ) -> Result<Response> {
        let body = Request {
            state,
            model: &self.model,
            questions,
        };
        let mut attempt = 0;
        loop {
            attempt += 1;
            let backoff = Duration::from_millis(400 * 2u64.pow(attempt - 1));
            let resp = self
                .http
                .post(ENDPOINT)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .await;

            match resp {
                Ok(r) if r.status().is_success() => {
                    return r.json::<Response>().await.context("decoding Jev response");
                }
                Ok(r)
                    if attempt < MAX_ATTEMPTS
                        && (r.status() == 429
                            || r.status() == 529
                            || r.status().is_server_error()) =>
                {
                    tokio::time::sleep(backoff).await;
                }
                Ok(r) => {
                    let status = r.status();
                    let text = r.text().await.unwrap_or_default();
                    bail!("Jev returned {status}: {text}");
                }
                Err(_) if attempt < MAX_ATTEMPTS => tokio::time::sleep(backoff).await,
                Err(e) => return Err(e).context("calling Jev"),
            }
        }
    }
}
