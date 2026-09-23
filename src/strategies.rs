//! Every strategy answers "what does FizzBuzz print for n?" by asking Jev.
//!
//! Two axes vary independently: the **strategy** (which questions are asked and
//! how answers are combined) and the **data form** (how the number is written).

use std::collections::BTreeMap;
use std::fmt;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::classic;
use crate::jev::{Answer, Question};

pub struct Verdict {
    pub output: String,
    pub notes: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Baseline,
    Phrasing,
    Choice,
    Outsource,
}

impl Family {
    pub fn all() -> [Family; 4] {
        [
            Family::Baseline,
            Family::Phrasing,
            Family::Choice,
            Family::Outsource,
        ]
    }

    pub fn title(self) -> &'static str {
        match self {
            Family::Baseline => "Baselines: no model, just a coin or a shrug",
            Family::Phrasing => {
                "Yes/no questions: the same three nouls, with more and more context"
            }
            Family::Choice => "Four-way choice: \"what gets printed?\", framed two ways",
            Family::Outsource => "Outsourced arithmetic: things the docs told us not to do",
        }
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Family::Baseline => "baseline",
            Family::Phrasing => "phrasing",
            Family::Choice => "choice",
            Family::Outsource => "outsource",
        };
        f.write_str(s)
    }
}

/// How the number is written into the state, independent of the strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum DataForm {
    /// Plain decimal: 42
    Number,
    /// English: "forty-two"
    Words,
    /// Binary digits, least significant first, as in the 2016 post: [0,1,0,1,0,1,0,0,0,0]
    Binary,
}

impl DataForm {
    pub fn name(self) -> &'static str {
        match self {
            DataForm::Number => "number",
            DataForm::Words => "words",
            DataForm::Binary => "binary",
        }
    }

    fn present(self, n: u32, bits: u32) -> Value {
        match self {
            DataForm::Number => json!(n),
            DataForm::Words => json!(classic::words(n)),
            DataForm::Binary => json!(classic::binary_digits(n, bits)),
        }
    }

    fn context(self, bits: u32) -> Map<String, Value> {
        match self {
            DataForm::Number => Map::new(),
            DataForm::Words => obj([("number_encoding", json!("written out in English words"))]),
            DataForm::Binary => obj([(
                "number_encoding",
                json!(format!(
                    "{bits} binary digits, least significant digit first"
                )),
            )]),
        }
    }
}

impl fmt::Display for DataForm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

pub trait Strategy: Send + Sync {
    fn name(&self) -> &'static str;
    fn family(&self) -> Family;

    /// Question type tag for tables: `y/n ×3`, `choice`, `score`, or `none` for baselines.
    fn kind(&self) -> &'static str;

    fn label(&self) -> &'static str;

    /// State that does not depend on the number.
    fn context(&self) -> Map<String, Value> {
        Map::new()
    }

    /// Empty for baselines, which never call the model.
    fn questions(&self) -> BTreeMap<String, Question>;

    fn uses_model(&self) -> bool {
        !self.questions().is_empty()
    }

    fn decide(&self, n: u32, answers: &BTreeMap<String, Answer>) -> Result<Verdict>;
}

pub fn all() -> Vec<Box<dyn Strategy>> {
    // Display order; the phrasing ladder runs vaguest first.
    vec![
        Box::new(AlwaysNumber),
        Box::new(Random),
        Box::new(WeightedRandom),
        Box::new(VIBES),
        Box::new(VIBES_GAME),
        Box::new(VIBES_RULES),
        Box::new(Divisible),
        Box::new(Print),
        Box::new(Classic),
        Box::new(Modulo),
        Box::new(Fizziness),
    ]
}

pub fn by_name(name: &str) -> Option<Box<dyn Strategy>> {
    all().into_iter().find(|s| s.name() == name)
}

/// The 2016 post trained on 101..=1023, so the classic test set 1..=100 stays unseen.
pub const EXAMPLE_POOL: std::ops::RangeInclusive<u32> = 101..=1023;

pub fn build_state(
    strategy: &dyn Strategy,
    data: DataForm,
    bits: u32,
    n: u32,
    examples: usize,
) -> Value {
    let mut state = strategy.context();
    state.extend(data.context(bits));
    let pool = (EXAMPLE_POOL.end() - EXAMPLE_POOL.start() + 1) as usize;
    if let Some(stride) = pool.checked_div(examples) {
        let stride = stride.max(1) as u32;
        let solved: Vec<Value> = (0..examples)
            .map(|i| EXAMPLE_POOL.start() + i as u32 * stride)
            .filter(|k| EXAMPLE_POOL.contains(k))
            .map(|k| {
                json!({
                    "number": data.present(k, bits),
                    "fizzbuzz": classic::fizzbuzz(k),
                })
            })
            .collect();
        state.insert("solved_examples".into(), Value::Array(solved));
    }
    state.insert("number".into(), data.present(n, bits));
    Value::Object(state)
}

/// splitmix64 of (n, salt) mapped to [0, 1), so "random" baselines are reproducible.
fn unit_draw(n: u32, salt: u64) -> f64 {
    let mut z = (n as u64)
        .wrapping_add(salt)
        .wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

fn pick(n: u32, u: f64, cuts: [f64; 3]) -> String {
    if u < cuts[0] {
        n.to_string()
    } else if u < cuts[1] {
        "Fizz".to_string()
    } else if u < cuts[2] {
        "Buzz".to_string()
    } else {
        "FizzBuzz".to_string()
    }
}

pub struct AlwaysNumber;

impl Strategy for AlwaysNumber {
    fn name(&self) -> &'static str {
        "always-number"
    }
    fn family(&self) -> Family {
        Family::Baseline
    }
    fn kind(&self) -> &'static str {
        "none"
    }
    fn label(&self) -> &'static str {
        "Always print the number"
    }
    fn questions(&self) -> BTreeMap<String, Question> {
        BTreeMap::new()
    }
    fn decide(&self, n: u32, _: &BTreeMap<String, Answer>) -> Result<Verdict> {
        Ok(Verdict {
            output: n.to_string(),
            notes: "no model".to_string(),
        })
    }
}

pub struct Random;

impl Strategy for Random {
    fn name(&self) -> &'static str {
        "random"
    }
    fn family(&self) -> Family {
        Family::Baseline
    }
    fn kind(&self) -> &'static str {
        "none"
    }
    fn label(&self) -> &'static str {
        "Pick one of {number, Fizz, Buzz, FizzBuzz} uniformly at random"
    }
    fn questions(&self) -> BTreeMap<String, Question> {
        BTreeMap::new()
    }
    fn decide(&self, n: u32, _: &BTreeMap<String, Answer>) -> Result<Verdict> {
        let u = unit_draw(n, 1);
        Ok(Verdict {
            output: pick(n, u, [0.25, 0.5, 0.75]),
            notes: format!("u={u:.2}"),
        })
    }
}

pub struct WeightedRandom;

impl Strategy for WeightedRandom {
    fn name(&self) -> &'static str {
        "random-weighted"
    }
    fn family(&self) -> Family {
        Family::Baseline
    }
    fn kind(&self) -> &'static str {
        "none"
    }
    fn label(&self) -> &'static str {
        "Pick at random with the true odds: 8/15 number, 4/15 Fizz, 2/15 Buzz, 1/15 FizzBuzz"
    }
    fn questions(&self) -> BTreeMap<String, Question> {
        BTreeMap::new()
    }
    fn decide(&self, n: u32, _: &BTreeMap<String, Answer>) -> Result<Verdict> {
        let u = unit_draw(n, 2);
        Ok(Verdict {
            output: pick(n, u, [8.0 / 15.0, 12.0 / 15.0, 14.0 / 15.0]),
            notes: format!("u={u:.2}"),
        })
    }
}

type NoulSpec = (&'static str, Option<(&'static str, &'static str)>);

/// Three nouls (fizz, buzz, fizzbuzz) whose wording is the only variable.
pub struct NoulTrio {
    name: &'static str,
    label: &'static str,
    fizz: NoulSpec,
    buzz: NoulSpec,
    fizzbuzz: NoulSpec,
}

pub const VIBES: NoulTrio = NoulTrio {
    name: "vibes",
    label: "Is this {fizz, buzz, fizzbuzz}?",
    fizz: ("Is this fizz?", None),
    buzz: ("Is this buzz?", None),
    fizzbuzz: ("Is this fizzbuzz?", None),
};

pub const VIBES_GAME: NoulTrio = NoulTrio {
    name: "vibes-game",
    label: "In FizzBuzz, is this number a {Fizz, Buzz, FizzBuzz}?",
    fizz: ("In the game FizzBuzz, is this number a Fizz?", None),
    buzz: ("In the game FizzBuzz, is this number a Buzz?", None),
    fizzbuzz: ("In the game FizzBuzz, is this number a FizzBuzz?", None),
};

pub const VIBES_RULES: NoulTrio = NoulTrio {
    name: "vibes-rules",
    label: "Is this {fizz, buzz, fizzbuzz}? (+ the divisibility rule as true/false criteria)",
    fizz: (
        "Is this fizz?",
        Some((
            "The number is divisible by 3 but not by 5.",
            "The number is not divisible by 3, or it is also divisible by 5.",
        )),
    ),
    buzz: (
        "Is this buzz?",
        Some((
            "The number is divisible by 5 but not by 3.",
            "The number is not divisible by 5, or it is also divisible by 3.",
        )),
    ),
    fizzbuzz: (
        "Is this fizzbuzz?",
        Some((
            "The number is divisible by both 3 and 5, i.e. by 15.",
            "The number is not divisible by 15.",
        )),
    ),
};

impl NoulTrio {
    fn question(spec: NoulSpec) -> Question {
        match spec {
            (q, None) => Question::noul(q),
            (q, Some((yes, no))) => Question::noul_with(q, yes, no),
        }
    }
}

impl Strategy for NoulTrio {
    fn name(&self) -> &'static str {
        self.name
    }
    fn family(&self) -> Family {
        Family::Phrasing
    }
    fn kind(&self) -> &'static str {
        "y/n ×3"
    }
    fn label(&self) -> &'static str {
        self.label
    }
    fn questions(&self) -> BTreeMap<String, Question> {
        qs([
            ("fizz", Self::question(self.fizz)),
            ("buzz", Self::question(self.buzz)),
            ("fizzbuzz", Self::question(self.fizzbuzz)),
        ])
    }
    fn decide(&self, n: u32, answers: &BTreeMap<String, Answer>) -> Result<Verdict> {
        let fizz = noul(answers, "fizz")?;
        let buzz = noul(answers, "buzz")?;
        let fizzbuzz = noul(answers, "fizzbuzz")?;
        // Separate nouls can disagree; fizzbuzz outranks fizz and buzz.
        let output = if fizzbuzz >= 0.5 || (fizz >= 0.5 && buzz >= 0.5) {
            "FizzBuzz".to_string()
        } else if fizz >= 0.5 {
            "Fizz".to_string()
        } else if buzz >= 0.5 {
            "Buzz".to_string()
        } else {
            n.to_string()
        };
        Ok(Verdict {
            output,
            notes: format!("fizz={fizz:.2} buzz={buzz:.2} fizzbuzz={fizzbuzz:.2}"),
        })
    }
}

pub struct Divisible;

impl Strategy for Divisible {
    fn name(&self) -> &'static str {
        "divisible"
    }
    fn family(&self) -> Family {
        Family::Phrasing
    }
    fn kind(&self) -> &'static str {
        "y/n ×2"
    }
    fn label(&self) -> &'static str {
        "Is the number divisible by {3, 5}?"
    }
    fn questions(&self) -> BTreeMap<String, Question> {
        qs([
            (
                "div3",
                Question::noul_with(
                    "Is the number divisible by 3 with no remainder?",
                    "The number is a multiple of 3, such as 3, 6, 9, 12, 15, 18, 21.",
                    "Dividing the number by 3 leaves a remainder of 1 or 2.",
                ),
            ),
            (
                "div5",
                Question::noul_with(
                    "Is the number divisible by 5 with no remainder?",
                    "The number is a multiple of 5, so it ends in 0 or 5.",
                    "The number does not end in 0 or 5.",
                ),
            ),
        ])
    }
    fn decide(&self, n: u32, answers: &BTreeMap<String, Answer>) -> Result<Verdict> {
        let p3 = noul(answers, "div3")?;
        let p5 = noul(answers, "div5")?;
        Ok(Verdict {
            output: label(n, p3 >= 0.5, p5 >= 0.5),
            notes: format!("div3={p3:.2} div5={p5:.2}"),
        })
    }
}

pub struct Print;

impl Strategy for Print {
    fn name(&self) -> &'static str {
        "print"
    }
    fn family(&self) -> Family {
        Family::Choice
    }
    fn kind(&self) -> &'static str {
        "choice"
    }
    fn label(&self) -> &'static str {
        "What does FizzBuzz print for the number? {number, Fizz, Buzz, FizzBuzz}"
    }
    fn questions(&self) -> BTreeMap<String, Question> {
        qs([(
            "output",
            output_choice("What does FizzBuzz print for the number?"),
        )])
    }
    fn decide(&self, n: u32, answers: &BTreeMap<String, Answer>) -> Result<Verdict> {
        decide_output_choice(n, answers, "output")
    }
}

pub struct Classic;

impl Strategy for Classic {
    fn name(&self) -> &'static str {
        "classic"
    }
    fn family(&self) -> Family {
        Family::Choice
    }
    fn kind(&self) -> &'static str {
        "choice"
    }
    fn label(&self) -> &'static str {
        "Given the interviewer's spec, what goes on the whiteboard? {number, Fizz, Buzz, FizzBuzz}"
    }
    fn context(&self) -> Map<String, Value> {
        obj([
            (
                "interviewer",
                json!(
                    "Print the numbers from 1 to 100, except that if the number is divisible by 3 print \"fizz\", if it's divisible by 5 print \"buzz\", and if it's divisible by 15 print \"fizzbuzz\"."
                ),
            ),
            (
                "candidate",
                json!(
                    "Whiteboard? That's the only way I code! I'll just write the output for this number."
                ),
            ),
        ])
    }
    fn questions(&self) -> BTreeMap<String, Question> {
        qs([(
            "whiteboard",
            output_choice(
                "Following the interviewer's rules exactly, what should the candidate write on the whiteboard for the number?",
            ),
        )])
    }
    fn decide(&self, n: u32, answers: &BTreeMap<String, Answer>) -> Result<Verdict> {
        decide_output_choice(n, answers, "whiteboard")
    }
}

pub struct Modulo;

impl Strategy for Modulo {
    fn name(&self) -> &'static str {
        "modulo"
    }
    fn family(&self) -> Family {
        Family::Outsource
    }
    fn kind(&self) -> &'static str {
        "choice ×2"
    }
    fn label(&self) -> &'static str {
        "What is the remainder when the number is divided by {3, 5}? {0, 1, 2} / {0..4}"
    }
    fn questions(&self) -> BTreeMap<String, Question> {
        qs([
            (
                "mod3",
                Question::choice(
                    "What is the remainder when the number is divided by 3?",
                    [
                        ("0", "The number divides evenly by 3"),
                        ("1", "Dividing by 3 leaves a remainder of 1"),
                        ("2", "Dividing by 3 leaves a remainder of 2"),
                    ],
                ),
            ),
            (
                "mod5",
                Question::choice(
                    "What is the remainder when the number is divided by 5?",
                    [
                        ("0", "The number divides evenly by 5 (ends in 0 or 5)"),
                        ("1", "Remainder 1 (ends in 1 or 6)"),
                        ("2", "Remainder 2 (ends in 2 or 7)"),
                        ("3", "Remainder 3 (ends in 3 or 8)"),
                        ("4", "Remainder 4 (ends in 4 or 9)"),
                    ],
                ),
            ),
        ])
    }
    fn decide(&self, n: u32, answers: &BTreeMap<String, Answer>) -> Result<Verdict> {
        let (r3, p3, c3) = choice(answers, "mod3")?;
        let (r5, p5, c5) = choice(answers, "mod5")?;
        let r3: u32 = r3.parse().context("mod3 was not a digit")?;
        let r5: u32 = r5.parse().context("mod5 was not a digit")?;
        Ok(Verdict {
            output: label(n, r3 == 0, r5 == 0),
            notes: format!(
                "n%3={r3} ({c3:.2}: {}) n%5={r5} ({c5:.2}: {})",
                fmt_probs(p3),
                fmt_probs(p5)
            ),
        })
    }
}

pub struct Fizziness;

/// Gray order: adjacent levels differ by one divisibility fact, so a score that
/// lands between two levels means one factor is uncertain, never both.
const FIZZINESS_LEVELS: [&str; 4] = [
    "Flat. Divisible by neither 3 nor 5. Prints as a plain number.",
    "Fizzy. Divisible by 3 but not by 5. Prints Fizz.",
    "Fizzy & Buzzy. Divisible by both 3 and 5. Prints FizzBuzz.",
    "Buzzy. Divisible by 5 but not by 3. Prints Buzz.",
];

impl Strategy for Fizziness {
    fn name(&self) -> &'static str {
        "fizziness"
    }
    fn family(&self) -> Family {
        Family::Outsource
    }
    fn kind(&self) -> &'static str {
        "score"
    }
    fn label(&self) -> &'static str {
        "How fizzy is the number? Flat < Fizzy < Fizzy & Buzzy < Buzzy"
    }
    fn questions(&self) -> BTreeMap<String, Question> {
        qs([(
            "fizziness",
            Question::score("How fizzy is the number?", FIZZINESS_LEVELS),
        )])
    }
    fn decide(&self, n: u32, answers: &BTreeMap<String, Answer>) -> Result<Verdict> {
        let (score, probs, conf) = score(answers, "fizziness")?;
        // `score` is the probability-weighted mean of the level indices.
        let level = score.round().clamp(0.0, 3.0) as usize;
        let output = match level {
            1 => "Fizz".to_string(),
            2 => "FizzBuzz".to_string(),
            3 => "Buzz".to_string(),
            _ => n.to_string(),
        };
        Ok(Verdict {
            output,
            notes: format!("fizziness={score} ({conf:.2}: {})", fmt_probs(probs)),
        })
    }
}

fn obj<'a>(pairs: impl IntoIterator<Item = (&'a str, Value)>) -> Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn qs<'a>(pairs: impl IntoIterator<Item = (&'a str, Question)>) -> BTreeMap<String, Question> {
    pairs.into_iter().map(|(k, q)| (k.to_string(), q)).collect()
}

fn label(n: u32, div3: bool, div5: bool) -> String {
    match (div3, div5) {
        (true, true) => "FizzBuzz".to_string(),
        (true, false) => "Fizz".to_string(),
        (false, true) => "Buzz".to_string(),
        (false, false) => n.to_string(),
    }
}

fn output_choice(instructions: &str) -> Question {
    Question::choice(
        instructions,
        [
            (
                "number",
                "The number itself, unchanged, because it is divisible by neither 3 nor 5.",
            ),
            ("Fizz", "The number is divisible by 3 but not by 5."),
            ("Buzz", "The number is divisible by 5 but not by 3."),
            (
                "FizzBuzz",
                "The number is divisible by both 3 and 5, i.e. by 15.",
            ),
        ],
    )
}

fn decide_output_choice(n: u32, answers: &BTreeMap<String, Answer>, key: &str) -> Result<Verdict> {
    let (pick, probs, conf) = choice(answers, key)?;
    let output = match pick {
        "number" => n.to_string(),
        other => other.to_string(),
    };
    Ok(Verdict {
        output,
        notes: format!("conf={conf:.2} {}", fmt_probs(probs)),
    })
}

fn noul(answers: &BTreeMap<String, Answer>, key: &str) -> Result<f64> {
    match answers.get(key) {
        Some(Answer::Noul { noul }) => Ok(*noul),
        Some(other) => bail!("expected a noul answer for {key}, got {other:?}"),
        None => bail!("no answer for {key}"),
    }
}

fn choice<'a>(
    answers: &'a BTreeMap<String, Answer>,
    key: &str,
) -> Result<(&'a str, &'a BTreeMap<String, f64>, f64)> {
    match answers.get(key) {
        Some(Answer::Choice {
            choice,
            probabilities,
            confidence,
        }) => Ok((choice, probabilities, *confidence)),
        Some(other) => bail!("expected a choice answer for {key}, got {other:?}"),
        None => bail!("no answer for {key}"),
    }
}

fn score<'a>(
    answers: &'a BTreeMap<String, Answer>,
    key: &str,
) -> Result<(f64, &'a BTreeMap<String, f64>, f64)> {
    match answers.get(key) {
        Some(Answer::Score {
            score,
            probabilities,
            confidence,
        }) => Ok((*score, probabilities, *confidence)),
        Some(other) => bail!("expected a score answer for {key}, got {other:?}"),
        None => bail!("no answer for {key}"),
    }
}

fn fmt_probs(probs: &BTreeMap<String, f64>) -> String {
    let mut sorted: Vec<_> = probs.iter().collect();
    sorted.sort_by(|a, b| b.1.total_cmp(a.1));
    sorted
        .iter()
        .map(|(k, v)| format!("{k}={v:.2}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_strategy_has_a_unique_name() {
        let names: Vec<_> = all().iter().map(|s| s.name()).collect();
        let mut dedup = names.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(names.len(), dedup.len());
    }

    #[test]
    fn baselines_never_call_the_model() {
        for s in all().iter().filter(|s| s.family() == Family::Baseline) {
            assert!(!s.uses_model(), "{} should not use the model", s.name());
            assert!(s.decide(7, &BTreeMap::new()).is_ok());
        }
    }

    #[test]
    fn solved_examples_stay_out_of_the_test_set() {
        let state = build_state(&Print, DataForm::Number, 10, 7, 12);
        let examples = state["solved_examples"].as_array().unwrap();
        assert_eq!(examples.len(), 12);
        for ex in examples {
            let k = ex["number"].as_u64().unwrap();
            assert!(
                EXAMPLE_POOL.contains(&(k as u32)),
                "{k} leaked from the test set"
            );
            assert_eq!(ex["fizzbuzz"], classic::fizzbuzz(k as u32));
        }
        assert_eq!(state["number"], 7);
    }

    #[test]
    fn data_forms_change_only_the_number() {
        let words = build_state(&Print, DataForm::Words, 10, 42, 0);
        assert_eq!(words["number"], "forty-two");
        let binary = build_state(&Print, DataForm::Binary, 10, 5, 0);
        assert_eq!(binary["number"], json!([1, 0, 1, 0, 0, 0, 0, 0, 0, 0]));
        let plain = build_state(&Print, DataForm::Number, 10, 7, 0);
        assert!(plain.get("solved_examples").is_none());
        assert!(plain.get("number_encoding").is_none());
    }

    #[test]
    fn vibes_resolves_contradictions_by_seniority() {
        let answers = BTreeMap::from([
            ("fizz".to_string(), Answer::Noul { noul: 0.9 }),
            ("buzz".to_string(), Answer::Noul { noul: 0.8 }),
            ("fizzbuzz".to_string(), Answer::Noul { noul: 0.1 }),
        ]);
        assert_eq!(VIBES.decide(7, &answers).unwrap().output, "FizzBuzz");
    }
}
