//! Startup module - demonstrates function-level suppression.
//!
//! These functions run before the async runtime is initialized,
//! so synchronous I/O is acceptable.

use std::fs;
use std::path::Path;

/// Load configuration at startup.
///
/// Sync I/O is acceptable here because we haven't started the async runtime yet.
#[arch_lint::allow(no_sync_io, reason = "Startup runs before async runtime")]
#[arch_lint::allow(no_unwrap_expect, reason = "Config file must exist")]
pub fn load_config() -> String {
    let config_path = Path::new("config.toml");

    if config_path.exists() {
        fs::read_to_string(config_path).unwrap_or_default()
    } else {
        String::from("default config")
    }
}

/// Initialize logging.
#[arch_lint::allow(no_sync_io, reason = "One-time startup operation")]
pub fn init_logging() {
    let log_dir = Path::new("logs");

    if !log_dir.exists() {
        let _ = fs::create_dir_all(log_dir);
    }
}
