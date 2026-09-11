// Fixture: every violation in this file is suppressed.
//
//   ALD001 restrict-use — comment directive naming the TOML rule
//   ALD005 deny-attr    — attribute on the offending item
//   ALD007 naming       — attribute naming the rule code, and one on an
//                         enclosing `mod`

// arch-lint: allow(no-sqlx-in-domain) reason="legacy row mapping"
use sqlx::Pool;

#[arch_lint::allow(no_orm_derives_in_domain, reason = "legacy row mapping")]
#[derive(sqlx::FromRow)]
pub struct User {
    pub id: i64,
}

#[arch_lint::allow(ALD007, reason = "renamed in v2")]
pub trait UserService {
    fn find(&self, id: i64) -> Option<User>;
}

#[arch_lint::allow(naming, reason = "legacy names, renamed in v2")]
pub mod legacy {
    pub trait AccountService {
        fn pool(&self) -> Option<Pool>;
    }
}
