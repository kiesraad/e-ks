mod client;
mod data;
mod metrics;
mod scenario;

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use clap::Parser;
use rand::{RngExt, distr::Alphanumeric};
use tokio::task::JoinSet;
use url::Url;

use crate::client::Client;
use crate::metrics::Collector;
use crate::scenario::{ScenarioConfig, run_session};

#[derive(Parser, Debug)]
#[command(about = "Concurrent session load test for the e-KS app")]
struct Args {
    /// Base URL of the server, e.g. http://localhost:3000
    #[arg(long, default_value = "http://localhost:3000")]
    base_url: String,

    /// Number of concurrent simulated users.
    #[arg(long, default_value_t = 10)]
    users: usize,

    /// Number of full sessions per user (each session re-logs in).
    #[arg(long, default_value_t = 1)]
    runs_per_user: usize,

    /// How many fixture persons each session should create + address.
    #[arg(long, default_value_t = 50)]
    persons_per_user: usize,

    /// Election to select. Must match an `ElectionConfig::type_options()` code.
    #[arg(long, default_value = "EK27")]
    election: String,

    /// Tick the "load fixtures" checkbox in the select-election form. Skip
    /// for a true cold-load — when set, the server pre-loads, which makes the
    /// per-session POSTs hit a non-empty store.
    #[arg(long)]
    load_fixtures: bool,

    /// How many times to shuffle + POST the candidate-list reorder endpoint.
    #[arg(long, default_value_t = 3)]
    reorders: usize,

    /// Per-request HTTP timeout in seconds. The H9 zip endpoint renders one
    /// PDF per candidate via Typst, so under heavy concurrency it can take
    /// well over a minute — bump this if you see "send failed after X.Xs"
    /// errors on `download:h9` or `download:h1`.
    #[arg(long, default_value_t = 300)]
    timeout_secs: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let base = Url::parse(&args.base_url).with_context(|| format!("parse base_url {}", args.base_url))?;
    let persons = data::load_persons().context("load persons.csv")?;
    println!(
        "loadtest: {} users x {} runs, {} persons/run against {}",
        args.users, args.runs_per_user, args.persons_per_user, args.base_url
    );
    if args.persons_per_user > persons.len() {
        anyhow::bail!(
            "persons_per_user ({}) > available fixture rows ({})",
            args.persons_per_user,
            persons.len()
        );
    }

    let (reporter, collector) = Collector::new();
    let scenario = Arc::new(ScenarioConfig {
        persons_per_user: args.persons_per_user,
        election: Box::leak(args.election.into_boxed_str()),
        load_fixtures_via_form: args.load_fixtures,
        reorders: args.reorders,
    });
    let persons = Arc::new(persons);

    let started = std::time::Instant::now();
    let args_timeout_secs = args.timeout_secs;
    let mut tasks: JoinSet<Result<()>> = JoinSet::new();
    for user in 0..args.users {
        let base = base.clone();
        let reporter = reporter.clone();
        let scenario = scenario.clone();
        let persons = persons.clone();
        let runs = args.runs_per_user;
        tasks.spawn(async move {
            for run in 0..runs {
                let suffix = format!(
                    "u{user}r{run}{}",
                    rand::rng()
                        .sample_iter(&Alphanumeric)
                        .take(4)
                        .map(char::from)
                        .collect::<String>()
                );
                let mut client = Client::new(
                    base.clone(),
                    reporter.clone(),
                    Duration::from_secs(args_timeout_secs),
                )?;
                if let Err(err) = run_session(&mut client, &persons, &suffix, &scenario).await {
                    eprintln!("user={user} run={run}: {err:#}");
                }
            }
            Ok(())
        });
    }

    while let Some(joined) = tasks.join_next().await {
        if let Err(err) = joined {
            eprintln!("task panicked: {err}");
        }
    }
    drop(reporter);
    let summary = collector.drain().await;
    summary.print();
    println!("wall clock: {:.2}s", started.elapsed().as_secs_f64());
    Ok(())
}
