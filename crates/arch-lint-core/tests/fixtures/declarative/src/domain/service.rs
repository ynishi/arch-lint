// Violation 1: restrict-use (sqlx in domain)
use sqlx::Pool;

// Violation 2: deny-scope-dep (domain -> infra)
use crate::infra::db::Connection;

pub fn run() {}
