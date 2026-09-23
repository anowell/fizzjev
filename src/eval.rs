//! Eval outputs: the leaderboard text and the JSON dump the HTML report is built from.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::{Context as _, Result};
use serde::Serialize;

use crate::Report;
use crate::jev::Question;
use crate::report;
use crate::strategies::{self, DataForm, Family};

pub const TITLE: &str = "FizzBuzz question eval";

pub struct Context {
    pub model: String,
    pub from: u32,
    pub to: u32,
    pub examples: usize,
    pub data: Vec<DataForm>,
    pub generated_unix: u64,
}

const CLASSES: [&str; 4] = ["number", "Fizz", "Buzz", "FizzBuzz"];

fn class_of(expected: &str) -> &'static str {
    match expected {
        "Fizz" => "Fizz",
        "Buzz" => "Buzz",
        "FizzBuzz" => "FizzBuzz",
        _ => "number",
    }
}

/// Per-class (correct, total) for one report.
fn per_class(report: &Report) -> BTreeMap<&'static str, (usize, usize)> {
    let mut m: BTreeMap<&'static str, (usize, usize)> =
        CLASSES.iter().map(|c| (*c, (0, 0))).collect();
    for row in &report.rows {
        let e = m.get_mut(class_of(&row.expected)).expect("known class");
        e.1 += 1;
        if row.correct {
            e.0 += 1;
        }
    }
    m
}

pub fn render(reports: &[Report], ctx: &Context) -> String {
    let mut out = String::new();
    let total = reports.first().map(|r| r.rows.len()).unwrap_or(0);
    let multi = ctx.data.len() > 1;

    let tokens: u64 = reports.iter().map(Report::tokens).sum();
    let secs: f64 = reports.iter().map(|r| r.elapsed.as_secs_f64()).sum();
    let _ = writeln!(
        out,
        "model {} · n = {}..={} · {} runs · {} tokens · {:.0}s{}",
        ctx.model,
        ctx.from,
        ctx.to,
        reports.len(),
        tokens,
        secs,
        if ctx.examples > 0 {
            format!(" · {} solved examples in state", ctx.examples)
        } else {
            String::new()
        }
    );
    out.push('\n');

    for family in Family::all() {
        let mut group: Vec<&Report> = reports.iter().filter(|r| r.family == family).collect();
        if group.is_empty() {
            continue;
        }
        group.sort_by(|a, b| b.accuracy().total_cmp(&a.accuracy()));
        let _ = writeln!(out, "## {}", family.title());
        out.push('\n');
        let width = group
            .iter()
            .map(|r| r.label.chars().count())
            .max()
            .unwrap_or(8)
            .max(8);
        let data_col = if multi { "| data   " } else { "" };
        let data_sep = if multi { "|--------" } else { "" };
        let _ = writeln!(
            out,
            "| {:<9} {data_col}| {:<width$} | {:>8} | {:>6} | {:>6} | {:>6} | {:>8} | {:<15} |",
            "asks", "question", "accuracy", "number", "Fizz", "Buzz", "FizzBuzz", "name"
        );
        let _ = writeln!(
            out,
            "|{}{data_sep}|{}|{}|{}|{}|{}|{}|{}|",
            "-".repeat(11),
            "-".repeat(width + 2),
            "-".repeat(10),
            "-".repeat(8),
            "-".repeat(8),
            "-".repeat(8),
            "-".repeat(10),
            "-".repeat(17)
        );
        for r in group {
            let pc = per_class(r);
            let cell = |c: &str| {
                let (ok, n) = pc[c];
                format!("{ok}/{n}")
            };
            let data_cell = if multi {
                format!("| {:<6} ", r.data.name())
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                "| {:<9} {data_cell}| {:<width$} | {:>7.1}% | {:>6} | {:>6} | {:>6} | {:>8} | {:<15} |",
                r.kind,
                r.label,
                r.accuracy() * 100.0,
                cell("number"),
                cell("Fizz"),
                cell("Buzz"),
                cell("FizzBuzz"),
                r.strategy
            );
        }
        out.push('\n');
    }

    for &form in &ctx.data {
        // Random baselines are noise, not disagreement.
        let cols: Vec<&Report> = reports
            .iter()
            .filter(|r| r.data == form && r.family != Family::Baseline)
            .collect();
        let wrong_numbers: Vec<u32> = {
            let mut v: Vec<u32> = cols
                .iter()
                .flat_map(|r| r.rows.iter().filter(|row| !row.correct).map(|row| row.n))
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        if wrong_numbers.is_empty() {
            let _ = writeln!(
                out,
                "Every strategy got every number right under `{form}`. Suspicious.\n"
            );
            continue;
        }
        if wrong_numbers.len() > 400 {
            let _ = writeln!(
                out,
                "## Where they disagree under `{form}`: {} of {} numbers (grid omitted; see the HTML report)\n",
                wrong_numbers.len(),
                total
            );
            continue;
        }
        let _ = writeln!(
            out,
            "## Where they disagree under `{form}` ({} of {} numbers; · means correct)",
            wrong_numbers.len(),
            total
        );
        out.push('\n');
        let mut header = format!("| {:>6} | {:<8} |", "n", "expected");
        let mut sep = format!("|{}|{}|", "-".repeat(8), "-".repeat(10));
        for r in &cols {
            let _ = write!(header, " {:<10} |", short(r.strategy));
            let _ = write!(sep, "{}|", "-".repeat(12));
        }
        let _ = writeln!(out, "{header}");
        let _ = writeln!(out, "{sep}");
        for n in &wrong_numbers {
            let expected = crate::classic::fizzbuzz(*n);
            let mut line = format!("| {n:>6} | {expected:<8} |");
            for r in &cols {
                let row = r.rows.iter().find(|row| row.n == *n);
                let cell = match row {
                    Some(row) if row.correct => "·".to_string(),
                    Some(row) => row.got.clone(),
                    None => "?".to_string(),
                };
                let _ = write!(line, " {cell:<10} |");
            }
            let _ = writeln!(out, "{line}");
        }
        out.push('\n');
    }

    let best = reports
        .iter()
        .filter(|r| r.family != Family::Baseline)
        .map(|r| r.accuracy())
        .fold(0.0_f64, f64::max);
    let _ = writeln!(out, "{}", punchline(best));
    out
}

#[derive(Serialize)]
struct LatestJson<'a> {
    generated_unix: u64,
    model: &'a str,
    from: u32,
    to: u32,
    examples: usize,
    data: &'a [DataForm],
    strategies: Vec<StrategyInfo>,
    runs: Vec<RunDump<'a>>,
}

#[derive(Serialize)]
struct StrategyInfo {
    name: &'static str,
    family: Family,
    kind: &'static str,
    label: &'static str,
    questions: BTreeMap<String, Question>,
}

#[derive(Serialize)]
struct RunDump<'a> {
    strategy: &'static str,
    data: DataForm,
    accuracy: f64,
    correct: usize,
    total: usize,
    tokens: u64,
    secs: f64,
    per_class: BTreeMap<&'static str, (usize, usize)>,
    rows: &'a [crate::Row],
}

fn first_reports(reports: &[Report]) -> Vec<&Report> {
    let mut seen: Vec<&Report> = Vec::new();
    for r in reports {
        if !seen.iter().any(|s| s.strategy == r.strategy) {
            seen.push(r);
        }
    }
    seen
}

pub fn to_json(reports: &[Report], ctx: &Context) -> Result<String> {
    let strategies = first_reports(reports)
        .iter()
        .filter_map(|r| strategies::by_name(r.strategy))
        .map(|s| StrategyInfo {
            name: s.name(),
            family: s.family(),
            kind: s.kind(),
            label: s.label(),
            questions: s.questions(),
        })
        .collect();
    let runs = reports
        .iter()
        .map(|r| RunDump {
            strategy: r.strategy,
            data: r.data,
            accuracy: r.accuracy(),
            correct: r.correct(),
            total: r.rows.len(),
            tokens: r.tokens(),
            secs: r.elapsed.as_secs_f64(),
            per_class: per_class(r),
            rows: &r.rows,
        })
        .collect();
    let dump = LatestJson {
        generated_unix: ctx.generated_unix,
        model: &ctx.model,
        from: ctx.from,
        to: ctx.to,
        examples: ctx.examples,
        data: &ctx.data,
        strategies,
        runs,
    };
    Ok(serde_json::to_string(&dump)?)
}

pub fn write_outputs(dir: &Path, reports: &[Report], ctx: &Context) -> Result<Vec<String>> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let json = to_json(reports, ctx)?;

    let mut md = format!("# {TITLE}\n\n");
    let _ = writeln!(md, "Generated at unix time {}.\n", ctx.generated_unix);
    md.push_str("## Strategies\n\n");
    for r in first_reports(reports) {
        let _ = writeln!(
            md,
            "- `{}` ({}, {}): {}",
            r.strategy, r.family, r.kind, r.label
        );
    }
    md.push('\n');
    md.push_str(&render(reports, ctx));

    let html = report::render_html(&json)?;

    let mut written = Vec::new();
    for (name, body) in [
        ("latest.json", json),
        ("latest.md", md),
        ("latest.html", html),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, body).with_context(|| format!("writing {}", path.display()))?;
        written.push(path.display().to_string());
    }
    Ok(written)
}

fn short(name: &str) -> String {
    name.replace("vibes-", "v-")
        .replace("random-", "r-")
        .replace("always-", "a-")
}

pub fn punchline(accuracy: f64) -> &'static str {
    if accuracy >= 1.0 {
        "Perfect score. Ship it. Do not tell anyone how it works."
    } else if accuracy >= 0.9 {
        "Close. I guess maybe I should have asked a deeper question."
    } else if accuracy >= 0.6 {
        "I guess maybe I should have used a deeper network."
    } else {
        "Interviewer: ...\nCandidate: I guess maybe I should have used a deeper network."
    }
}
