//! FizzBuzz, but every decision is outsourced to a calibrated decision model.
//!
//! With apologies to Joel Grus, "Fizz Buzz in TensorFlow" (2016).

mod classic;
mod eval;
mod jev;
mod report;
mod strategies;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use clap::{Args, Parser, Subcommand};
use futures::{StreamExt, stream};
use serde::Serialize;

use crate::jev::Client;
use crate::strategies::{DataForm, Family, Strategy};

#[derive(Parser, Debug)]
#[command(
    name = "fizzjev",
    about = "FizzBuzz, but every decision is outsourced to a calibrated decision model.",
    long_about = "FizzBuzz, but every decision is outsourced to a calibrated decision model.\n\n\
        `fizzbuzz` is the implementation you would write in an interview.\n\
        Everything else is what you would write in a *TensorFlow* interview.\n\n\
        Requires TYPESAFE_API_KEY (use `just ...`, which loads it)."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// The correct implementation. n % 3 and n % 5. Zero tokens. Zero fun.
    Fizzbuzz {
        #[command(flatten)]
        range: Range,
    },
    /// List the strategies and the question each one asks
    List,
    /// Run one strategy and print every answer
    Run {
        /// Strategy name, see `list`
        strategy: String,
        /// How the number is written into the state
        #[arg(long, value_enum, default_value_t = DataForm::Number)]
        data: DataForm,
        #[command(flatten)]
        range: Range,
        #[command(flatten)]
        opts: JevOpts,
        /// Show the probabilities behind every answer, not just the wrong ones
        #[arg(short, long)]
        verbose: bool,
    },
    /// Evaluate every strategy (or a chosen few), rank them, and write results/
    Eval {
        /// Comma-separated strategy names to include (default: all)
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
        /// Data forms to run every strategy under, comma-separated
        #[arg(long, value_enum, value_delimiter = ',', default_value = "number")]
        data: Vec<DataForm>,
        /// Directory for latest.json, latest.md and latest.html
        #[arg(long, default_value = "results")]
        out: PathBuf,
        #[command(flatten)]
        range: Range,
        #[command(flatten)]
        opts: JevOpts,
    },
    /// Rebuild the HTML report from a saved eval (no API calls)
    Report {
        #[arg(long, default_value = "results/latest.json")]
        input: PathBuf,
        #[arg(long, default_value = "results/latest.html")]
        out: PathBuf,
    },
}

#[derive(Args, Debug, Clone, Copy)]
struct Range {
    #[arg(long, default_value_t = 1)]
    from: u32,
    #[arg(long, default_value_t = 100)]
    to: u32,
}

impl Range {
    fn check(&self) -> Result<()> {
        if self.from > self.to {
            bail!("--from must not exceed --to");
        }
        Ok(())
    }
}

impl JevOpts {
    fn warn_if_examples_overlap(&self, range: Range) {
        let pool = strategies::EXAMPLE_POOL;
        if self.examples > 0 && range.to >= *pool.start() && range.from <= *pool.end() {
            eprintln!(
                "warning: --examples are drawn from {}..={}, which overlaps --from {}..--to {}; \
                 some numbers under test appear solved in the state",
                pool.start(),
                pool.end(),
                range.from,
                range.to
            );
        }
    }
}

#[derive(Args, Debug, Clone)]
struct JevOpts {
    /// Put N solved examples (number -> FizzBuzz output) in the state before the question.
    /// Drawn from 101..=1023, so the test set 1..=100 stays unseen, as tradition demands.
    #[arg(long, default_value_t = 0)]
    examples: usize,

    /// Concurrent requests in flight
    #[arg(long, default_value_t = 16)]
    concurrency: usize,

    #[arg(long, default_value = "jev-latest")]
    model: String,
}

#[derive(Serialize)]
pub struct Row {
    pub n: u32,
    pub expected: String,
    pub got: String,
    pub correct: bool,
    pub notes: String,
    pub tokens: u64,
    /// Raw Jev answers, kept for tracing.
    pub answers: serde_json::Value,
}

#[derive(Serialize)]
pub struct Report {
    pub strategy: &'static str,
    pub family: Family,
    pub kind: &'static str,
    pub label: &'static str,
    pub data: DataForm,
    pub rows: Vec<Row>,
    #[serde(serialize_with = "secs")]
    pub elapsed: Duration,
}

fn secs<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(d.as_secs_f64())
}

impl Report {
    pub fn correct(&self) -> usize {
        self.rows.iter().filter(|r| r.correct).count()
    }
    pub fn accuracy(&self) -> f64 {
        if self.rows.is_empty() {
            return 0.0;
        }
        self.correct() as f64 / self.rows.len() as f64
    }
    pub fn tokens(&self) -> u64 {
        self.rows.iter().map(|r| r.tokens).sum()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Fizzbuzz { range } => {
            range.check()?;
            for n in range.from..=range.to {
                println!("{}", classic::fizzbuzz(n));
            }
        }
        Command::List => list(),
        Command::Run {
            strategy,
            data,
            range,
            opts,
            verbose,
        } => {
            range.check()?;
            opts.warn_if_examples_overlap(range);
            let Some(strategy) = strategies::by_name(&strategy) else {
                bail!("unknown strategy `{strategy}`; try `list`");
            };
            let client = client_if_needed(std::slice::from_ref(&strategy), &opts)?;
            let report = run(
                client,
                Arc::from(strategy),
                data,
                range,
                &opts,
                Some(verbose),
            )
            .await?;
            print_summary(&report);
            println!();
            println!("{}", eval::punchline(report.accuracy()));
        }
        Command::Eval {
            only,
            mut data,
            out,
            range,
            opts,
        } => {
            range.check()?;
            opts.warn_if_examples_overlap(range);
            let mut seen = Vec::new();
            data.retain(|d| {
                let fresh = !seen.contains(d);
                seen.push(*d);
                fresh
            });
            let mut selected = strategies::all();
            if !only.is_empty() {
                for name in &only {
                    if strategies::by_name(name).is_none() {
                        bail!("unknown strategy `{name}`; try `list`");
                    }
                }
                selected.retain(|s| only.iter().any(|o| o == s.name()));
            }
            let client = client_if_needed(&selected, &opts)?;
            let selected: Vec<Arc<dyn Strategy>> = selected.into_iter().map(Arc::from).collect();

            let mut reports = Vec::new();
            for (i, &form) in data.iter().enumerate() {
                for strategy in &selected {
                    // Baselines ignore the data form; run them once.
                    if !strategy.uses_model() && i > 0 {
                        continue;
                    }
                    eprint!("running {:<16} {:<7}", strategy.name(), form.name());
                    let r = run(
                        client.clone(),
                        Arc::clone(strategy),
                        form,
                        range,
                        &opts,
                        None,
                    )
                    .await?;
                    eprintln!(" {:>5.1}%", r.accuracy() * 100.0);
                    reports.push(r);
                }
            }
            let ctx = eval::Context {
                model: opts.model.clone(),
                from: range.from,
                to: range.to,
                examples: opts.examples,
                data: data.clone(),
                generated_unix: unix_now(),
            };
            println!();
            print!("{} · {}", eval::TITLE, eval::render(&reports, &ctx));
            let written = eval::write_outputs(&out, &reports, &ctx)?;
            eprintln!("wrote {}", written.join(", "));
        }
        Command::Report { input, out } => {
            let json = std::fs::read_to_string(&input)
                .with_context(|| format!("reading {}", input.display()))?;
            std::fs::write(&out, report::render_html(&json)?)
                .with_context(|| format!("writing {}", out.display()))?;
            eprintln!("wrote {}", out.display());
        }
    }
    Ok(())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn client_if_needed(
    strategies: &[Box<dyn Strategy>],
    opts: &JevOpts,
) -> Result<Option<Arc<Client>>> {
    if strategies.iter().any(|s| s.uses_model()) {
        Ok(Some(Arc::new(Client::from_env(&opts.model)?)))
    } else {
        Ok(None)
    }
}

fn list() {
    println!("{:<16} {:<10} {:<9} question", "name", "family", "asks");
    for s in strategies::all() {
        println!(
            "{:<16} {:<10} {:<9} {}",
            s.name(),
            s.family().to_string(),
            s.kind(),
            s.label()
        );
    }
    println!();
    println!("data forms: number (42), words (\"forty-two\"), binary ([0,1,0,1,0,1,0,0,0,0])");
}

/// `print_rows` is `Some(verbose)` to print each row as it arrives.
async fn run(
    client: Option<Arc<Client>>,
    strategy: Arc<dyn Strategy>,
    data: DataForm,
    range: Range,
    opts: &JevOpts,
    print_rows: Option<bool>,
) -> Result<Report> {
    let started = Instant::now();
    let questions = Arc::new(strategy.questions());
    let examples = opts.examples;
    let bits = classic::bits_for(range.to);
    if strategy.uses_model() && client.is_none() {
        bail!(
            "{} needs the model but no client was built",
            strategy.name()
        );
    }

    let mut results = stream::iter(range.from..=range.to)
        .map(|n| {
            let client = client.clone();
            let strategy = Arc::clone(&strategy);
            let questions = Arc::clone(&questions);
            async move {
                let (answers, tokens) = match client.filter(|_| !questions.is_empty()) {
                    Some(client) => {
                        let state =
                            strategies::build_state(strategy.as_ref(), data, bits, n, examples);
                        let resp = client.ask(&state, &questions).await?;
                        (resp.answers, resp.usage.total())
                    }
                    None => (Default::default(), 0),
                };
                let verdict = strategy.decide(n, &answers)?;
                let expected = classic::fizzbuzz(n);
                Ok::<_, anyhow::Error>(Row {
                    n,
                    correct: expected == verdict.output,
                    expected,
                    got: verdict.output,
                    notes: verdict.notes,
                    tokens,
                    answers: serde_json::to_value(&answers)?,
                })
            }
        })
        .buffered(opts.concurrency.max(1));

    let mut rows = Vec::new();
    while let Some(row) = results.next().await {
        let row = row?;
        if let Some(verbose) = print_rows {
            print_row(&row, verbose);
        }
        rows.push(row);
    }

    Ok(Report {
        strategy: strategy.name(),
        family: strategy.family(),
        kind: strategy.kind(),
        label: strategy.label(),
        data,
        rows,
        elapsed: started.elapsed(),
    })
}

fn print_row(row: &Row, verbose: bool) {
    if row.correct {
        let notes = if verbose { row.notes.as_str() } else { "" };
        println!("{:>6}  {:<10} ✓  {notes}", row.n, row.got);
    } else {
        println!(
            "{:>6}  {:<10} ✗  wanted {:<10} {}",
            row.n, row.got, row.expected, row.notes
        );
    }
}

fn print_summary(report: &Report) {
    println!();
    println!(
        "{} ({}): {}/{} correct ({:.1}%) · {} tokens · {:.1}s",
        report.strategy,
        report.data,
        report.correct(),
        report.rows.len(),
        report.accuracy() * 100.0,
        report.tokens(),
        report.elapsed.as_secs_f64()
    );
}
