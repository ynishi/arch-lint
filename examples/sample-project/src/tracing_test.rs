// Test cases for new tracing rules

use tracing_subscriber::EnvFilter;

// This should trigger AL006: require-tracing
pub fn old_logging() {
    log::info!("This is old-style logging");
    log::error!("This is an error");
}

// This should trigger AL007: tracing-env-init
pub fn hardcoded_init() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("debug"))
        .init();
}

// Good patterns
pub fn good_logging() {
    tracing::info!("This is tracing-style logging");
    tracing::error!("This is an error");
}

pub fn good_init() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
}
