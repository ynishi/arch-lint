// Fixture: the control file. Nothing here is suppressed, so the naming rule
// must still fire — proof that the rules are active in this fixture at all.

pub trait PlainService {
    fn find(&self, id: i64) -> Option<i64>;
}
