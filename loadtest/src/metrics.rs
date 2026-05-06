use std::collections::BTreeMap;
use std::time::Duration;

use tokio::sync::mpsc;

#[derive(Clone, Copy)]
pub struct Metric {
    pub label: &'static str,
    pub method: &'static str,
    pub status: u16,
    pub duration: Duration,
}

#[derive(Clone)]
pub struct Reporter {
    tx: mpsc::UnboundedSender<Metric>,
}

impl Reporter {
    pub fn record(&self, m: Metric) {
        let _ = self.tx.send(m);
    }
}

pub struct Collector {
    rx: mpsc::UnboundedReceiver<Metric>,
}

impl Collector {
    pub fn new() -> (Reporter, Self) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Reporter { tx }, Collector { rx })
    }

    pub async fn drain(mut self) -> Summary {
        let mut by_label: BTreeMap<(&'static str, &'static str), Vec<Metric>> = BTreeMap::new();
        while let Some(m) = self.rx.recv().await {
            by_label.entry((m.method, m.label)).or_default().push(m);
        }
        Summary { by_label }
    }
}

pub struct Summary {
    by_label: BTreeMap<(&'static str, &'static str), Vec<Metric>>,
}

impl Summary {
    pub fn print(&self) {
        println!(
            "\n{:<8} {:<32} {:>8} {:>10} {:>10} {:>10} {:>10} {:>8}",
            "method", "label", "count", "p50", "p90", "p99", "max", "errors"
        );
        println!("{}", "-".repeat(98));
        let mut total = 0usize;
        let mut errors = 0usize;
        for ((method, label), metrics) in &self.by_label {
            let mut durs: Vec<u128> = metrics.iter().map(|m| m.duration.as_micros()).collect();
            durs.sort_unstable();
            let count = durs.len();
            let p = |q: f64| -> u128 {
                let idx = ((count as f64 - 1.0) * q).round() as usize;
                durs[idx]
            };
            let err = metrics.iter().filter(|m| m.status >= 400).count();
            total += count;
            errors += err;
            println!(
                "{:<8} {:<32} {:>8} {:>10} {:>10} {:>10} {:>10} {:>8}",
                method,
                label,
                count,
                fmt_us(p(0.50)),
                fmt_us(p(0.90)),
                fmt_us(p(0.99)),
                fmt_us(*durs.last().unwrap()),
                err
            );
        }
        println!("{}", "-".repeat(98));
        println!("total requests: {total}, errors: {errors}");
    }
}

fn fmt_us(us: u128) -> String {
    if us >= 1_000_000 {
        format!("{:.2}s", us as f64 / 1_000_000.0)
    } else if us >= 1_000 {
        format!("{:.1}ms", us as f64 / 1_000.0)
    } else {
        format!("{us}us")
    }
}
