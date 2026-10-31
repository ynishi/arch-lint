use sqlx::Pool;

pub fn connect() { let _ = Pool::connect(); }
