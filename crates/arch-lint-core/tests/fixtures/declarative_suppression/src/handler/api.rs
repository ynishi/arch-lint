// Fixture: handler layer.
//
//   `stream_users` — ALD006 and ALD008 both suppressed by one attribute
//                    listing the two TOML rule names
//   `list_users`   — ALD008 is an error rule and the comment directive carries
//                    no reason, so a "missing required reason" warning is
//                    reported instead of the violation

use crate::infra::db::Pool; // downward dependency: allowed by [[layering]]

#[arch_lint::allow(
    instrument_handlers,
    handlers_return_app_result,
    reason = "streaming response, not a Result"
)]
pub async fn stream_users(_pool: Pool) -> String {
    String::new()
}

#[tracing::instrument]
// arch-lint: allow(handlers-return-app-result)
pub async fn list_users() -> String {
    String::new()
}
