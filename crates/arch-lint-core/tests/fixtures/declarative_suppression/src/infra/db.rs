// Fixture: infra layer, reaching upward into handler twice.
//
//   the `use` — ALD004 suppressed by an attribute on the `use` item itself
//   `wire()`  — ALD004 on an inline path, suppressed by the attribute on the
//               enclosing `fn`

#[arch_lint::allow(clean_arch, reason = "composition root wires the layers")]
use crate::handler::api::list_users;

pub struct Pool;

#[arch_lint::allow(layering, reason = "composition root wires the layers")]
pub fn wire() {
    let _ = list_users;
    let _ = crate::handler::api::stream_users;
}
