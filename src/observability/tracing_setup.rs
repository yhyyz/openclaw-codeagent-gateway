//! Structured logging initialization.

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub fn init_tracing(log_level: &str, json_format: bool) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    if json_format {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().pretty())
            .init();
    }
}
