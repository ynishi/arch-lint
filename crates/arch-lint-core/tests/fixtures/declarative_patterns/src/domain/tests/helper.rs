// Fixture: excluded from the `domain` scope via `exclude`.
//
// Everything here would be a violation if the file were in scope; the
// expectation is that no rule fires.

use sqlx::Pool;

#[derive(sqlx::FromRow)]
pub struct RowFixture {
    pub id: i64,
}

pub trait NotAPortName {
    fn pool(&self) -> Pool;
}
