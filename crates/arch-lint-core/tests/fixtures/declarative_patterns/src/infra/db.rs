// Fixture: infra layer.
//
// Expected violations:
//   ALD004 layering — infra reaches upward into handler

use crate::handler::api::list_users;

pub struct Pool;

pub fn wire() {
    let _ = list_users;
}
