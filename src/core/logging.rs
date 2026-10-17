//! Tracing subscriber initialization for application logging.
//! Called during startup to set log filters and formatting.
//!
//! The filter comes from `RUST_LOG` (tracing's own convention), the format
//! from [`Config::log_format`](crate::Config::log_format).

use serde::Deserialize;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// How log events are written (`LOG_FORMAT`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable lines.
    #[default]
    Pretty,
    /// One JSON object per event on stdout (intended for ingestion by a Loki
    /// agent).
    Json,
}

pub fn init(format: LogFormat) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        format!(
            "{}=info,tower_http=info,auth_service=info",
            env!("CARGO_CRATE_NAME")
        )
        .into()
    });

    let json = format == LogFormat::Json;

    println!(
        "Logging filter: {filter} (format: {})",
        if json { "json" } else { "pretty" }
    );

    let registry = tracing_subscriber::registry().with(filter);

    if json {
        registry
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .flatten_event(true)
                    .with_current_span(true)
                    .with_span_list(false),
            )
            .init();
    } else {
        registry.with(tracing_subscriber::fmt::layer()).init();
    }
}
