// Fixture: handler layer.
//
// Expected violations:
//   ALD006 require-attr  — `list_users` has no `#[tracing::instrument]`
//   ALD008 require-return — `list_users` returns String, not Result<_, AppError>

use crate::infra::db::Pool; // downward dependency: allowed by [[layering]]

pub async fn list_users(_pool: Pool) -> String {
    String::new()
}
