use chrono::{self, Utc};
use uuid::Uuid;
use uuid::Uuid as U;

pub fn by_name() -> Uuid { Uuid::new_v7() }
pub fn by_rename() -> Uuid { U::new_v7() }
pub fn by_group() { let _ = Utc::now(); }
pub fn spelled_uuid() -> uuid::Uuid { uuid::Uuid::new_v7() }
pub fn spelled_chrono() { let _ = chrono::Utc::now(); }
