//! Undo and redo for profile edits: snapshots of the whole profile, taken
//! before each edit. Quick successive edits (typing a name, dragging a
//! slider) collapse into one step.

use std::time::{Duration, Instant};

use crate::profile::Profile;

const LIMIT: usize = 50;
/// Edits closer together than this undo as one.
const COALESCE: Duration = Duration::from_millis(800);

#[derive(Default)]
pub struct History {
    undo: Vec<(String, Profile)>,
    redo: Vec<(String, Profile)>,
    last: Option<(String, Instant)>,
}

impl History {
    /// Remember `before`, the profile `id` as it was before an edit.
    pub fn record(&mut self, id: &str, before: &Profile, now: Instant) {
        let recent = self.last.as_ref().is_some_and(|(l, t)| l == id && now.duration_since(*t) < COALESCE);
        self.last = Some((id.to_string(), now));
        self.redo.clear();
        if recent && !self.undo.is_empty() {
            return;
        }
        self.undo.push((id.to_string(), before.clone()));
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
    }

    /// The profile to restore, given what it looks like now.
    pub fn undo(&mut self, current: impl Fn(&str) -> Option<Profile>) -> Option<(String, Profile)> {
        let (id, p) = self.undo.pop()?;
        if let Some(now) = current(&id) {
            self.redo.push((id.clone(), now));
        }
        self.last = None;
        Some((id, p))
    }

    pub fn redo(&mut self, current: impl Fn(&str) -> Option<Profile>) -> Option<(String, Profile)> {
        let (id, p) = self.redo.pop()?;
        if let Some(now) = current(&id) {
            self.undo.push((id.clone(), now));
        }
        self.last = None;
        Some((id, p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(n: &str) -> Profile {
        Profile { name: n.into(), ..Default::default() }
    }

    #[test]
    fn undo_redo_and_coalescing() {
        let mut h = History::default();
        let t0 = Instant::now();
        h.record("p", &named("a"), t0);
        // Typing: these collapse into the first step.
        h.record("p", &named("ab"), t0 + Duration::from_millis(200));
        h.record("p", &named("abc"), t0 + Duration::from_millis(400));
        // A separate edit later.
        h.record("p", &named("abcd"), t0 + Duration::from_secs(5));

        let (_, p) = h.undo(|_| Some(named("abcde"))).unwrap();
        assert_eq!(p.name, "abcd");
        let (_, p) = h.undo(|_| Some(named("abcd"))).unwrap();
        assert_eq!(p.name, "a");
        assert!(h.undo.is_empty());
        let (_, p) = h.redo(|_| Some(named("a"))).unwrap();
        assert_eq!(p.name, "abcd");
        // A new edit drops the redo steps.
        h.record("p", &named("x"), t0 + Duration::from_secs(9));
        assert!(h.redo(|_| None).is_none());
    }

    #[test]
    fn limited() {
        let mut h = History::default();
        let t0 = Instant::now();
        for i in 0..80 {
            h.record("p", &named(&i.to_string()), t0 + Duration::from_secs(i));
        }
        let mut n = 0;
        while h.undo(|_| None).is_some() {
            n += 1;
        }
        assert_eq!(n, LIMIT);
    }
}
