// Fixture: domain layer.
//
// Expected violations:
//   ALD005 deny-attr — `User` derives `sqlx::FromRow`
//   ALD007 naming    — `UserService` is neither *Repository nor *Port
//
// `use sqlx::types::Json` is covered by the restrict-use `except` list,
// so it must NOT produce an ALD001 violation.

use sqlx::types::Json;

#[derive(Debug, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub meta: Json<String>,
}

pub trait UserService {
    fn find(&self, id: i64) -> Option<User>;
}
