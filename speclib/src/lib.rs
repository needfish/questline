mod item;
mod mnemonic;

use std::array;
use std::slice;


pub use item::{commitment_kind, count_direction, days_remaining, path_to_root, Commitment, Date, Direction, Event, EventKind, Item, ItemId, ProjectKind, Status, Store};
pub use mnemonic::{Mnemonic, MnemonicError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveError {
    UnknownItem,
    UnknownParent,
    SelfParent,
    Cycle,
    DeadlineExceeded,
}

#[derive(Clone, Copy)]
pub enum Step {
    Rising,
    Falling,
}

impl Store {
    #[must_use]
    pub const fn empty() -> Self {
        Self(Vec::new())
    }

    #[must_use]
    pub fn get(&self, id: &ItemId) -> Option<&Item> {
        self.0.iter().find(|line| line.id == *id)
    }

    #[must_use]
    pub fn contains(&self, id: &ItemId) -> bool {
        self.get(id).is_some()
    }

    pub fn add(&mut self, line: Item) {
        assert!(!self.contains(&line.id), "duplicate item id {}", line.id.0);
        assert!(
            line.parent.as_ref().is_none_or(|parent_id| self.contains(parent_id)),
            "unknown parent"
        );
        assert!(
            self.check_deadline(line.kind, line.parent.as_ref()).is_ok(),
            "deadline exceeds parent deadline for item {}",
            line.id.0
        );
        self.0.push(line);
    }

    pub fn iter(&self) -> slice::Iter<'_, Item> {
        self.0.iter()
    }

    #[must_use]
    pub fn position(&self, id: &ItemId) -> Option<usize> {
        self.0.iter().position(|line| line.id == *id)
    }

    pub fn set_notes(&mut self, id: &ItemId, notes: String) -> Result<(), MoveError> {
        let stored = self.0.iter_mut().find(|line| line.id == *id);
        let Some(line) = stored else { return Err(MoveError::UnknownItem) };
        line.notes = notes;
        Ok(())
    }

    pub fn set_deadline(&mut self, id: &ItemId, deadline: Date) -> Result<(), MoveError> {
        let target = self.get(id).ok_or(MoveError::UnknownItem)?;
        self.check_new_deadline(target, id, deadline)?;
        let stored = self.0.iter_mut().find(|line| line.id == *id);
        if let Some(line) = stored {
            line.kind = ProjectKind::Closed { deadline };
        }
        Ok(())
    }

    fn check_new_deadline(&self, line: &Item, id: &ItemId, deadline: Date) -> Result<(), MoveError> {
        let late_child = self
            .descendants(id)
            .iter()
            .any(|child| matches!(&child.kind, ProjectKind::Closed { deadline: child_deadline } if *child_deadline > deadline));
        if late_child {
            return Err(MoveError::DeadlineExceeded);
        }
        let Some(parent_id) = &line.parent else { return Ok(()) };
        let parent = self.get(parent_id).ok_or(MoveError::UnknownParent)?;
        let exceeds = matches!(&parent.kind, ProjectKind::Closed { deadline: parent_deadline } if deadline > *parent_deadline);
        if exceeds { Err(MoveError::DeadlineExceeded) } else { Ok(()) }
    }

    #[must_use]
    fn descendants(&self, root: &ItemId) -> Vec<&Item> {
        let mut stack = Vec::new();
        stack.push(root.clone());
        let mut found = Vec::new();
        while let Some(parent_id) = stack.pop() {
            let children = self.0.iter().filter(|line| line.parent.as_ref() == Some(&parent_id));
            for line in children {
                stack.push(line.id.clone());
                found.push(line);
            }
        }
        found
    }

    pub fn toggle(&mut self, id: &ItemId, today: Date) -> Result<(), MoveError> {
        let line = self.0.iter_mut().find(|line| line.id == *id).ok_or(MoveError::UnknownItem)?;
        line.status = if line.status == Status::Done { Status::Active } else { Status::Done };
        line.history.push(Event { on: today, kind: EventKind::Done, detail: String::new() });
        Ok(())
    }

    pub fn confirm(&mut self, id: &ItemId, today: Date) -> Result<(), MoveError> {
        let line = self.0.iter_mut().find(|line| line.id == *id).ok_or(MoveError::UnknownItem)?;
        if line.status != Status::Proposed { return Err(MoveError::UnknownItem); }
        line.status = Status::Active;
        line.history.push(Event { on: today, kind: EventKind::Confirmed, detail: String::new() });
        Ok(())
    }

    pub fn drop_item(&mut self, id: &ItemId, today: Date) -> Result<(), MoveError> {
        let line = self.0.iter_mut().find(|line| line.id == *id).ok_or(MoveError::UnknownItem)?;
        if line.status == Status::Done { return Err(MoveError::UnknownItem); }
        line.status = Status::Archived;
        line.history.push(Event { on: today, kind: EventKind::Dropped, detail: String::new() });
        Ok(())
    }

    pub fn skip(&mut self, id: &ItemId, today: Date) -> Result<(), MoveError> {
        let line = self.0.iter_mut().find(|line| line.id == *id).ok_or(MoveError::UnknownItem)?;
        line.history.push(Event { on: today, kind: EventKind::Skipped, detail: String::new() });
        Ok(())
    }

    #[must_use]
    pub fn skip_count(&self, id: &ItemId) -> usize {
        self.get(id).map_or(0, |line| line.history.iter().filter(|event| event.kind == EventKind::Skipped).count())
    }

    #[must_use]
    pub fn last_touched(&self, id: &ItemId) -> Option<Date> {
        Some(self.get(id)?.history.last()?.on)
    }

    #[must_use]
    pub fn decayed(&self, id: &ItemId, today: Date) -> bool {
        let Some(line) = self.get(id) else { return false };
        if line.status != Status::Active { return false; }
        if self.skip_count(id) >= 3 { return true; }
        idle_days(self, id, today).is_some_and(|days| days >= 14)
    }

    #[must_use]
    pub fn reason_text(&self, id: &ItemId, today: Date) -> String {
        let Some(line) = self.get(id) else { return String::new() };
        let mut parts = Vec::new();
        if let Some(days) = days_remaining(line, today) {
            let due = match days {
                0 => "due today".to_owned(),
                1.. => format!("due in {days} days"),
                _ => format!("{} days overdue", days.unsigned_abs()),
            };
            parts.push(due);
        }
        let children = self.0.iter().filter(|child| child.parent.as_ref() == Some(id) && child.status == Status::Active).count();
        if children > 0 {
            parts.push(format!("{children} active"));
        }
        let skips = self.skip_count(id);
        if skips >= 3 {
            parts.push(format!("skipped {skips} times"));
        }
        if idle_days(self, id, today).is_some_and(|days| days >= 14) {
            parts.push(format!("idle {} days", idle_days(self, id, today).unwrap_or_default()));
        }
        parts.join(" · ")
    }

    pub fn reorder(&mut self, id: &ItemId, step: Step) -> Result<(), MoveError> {
        let Some(current) = self.position(id) else { return Err(MoveError::UnknownItem) };
        let parent = self.0.get(current).and_then(|line| line.parent.clone());
        let same_parent = |line: &Item| line.parent.clone() == parent;
        let slot = match step {
            Step::Rising => (0..current).rev().find(|&index| self.0.get(index).is_some_and(same_parent)),
            Step::Falling => current
                .checked_add(1)
                .and_then(|start| (start..self.0.len()).find(|&index| self.0.get(index).is_some_and(same_parent))),
        };
        if let Some(found) = slot {
            let line = self.0.remove(current);
            self.0.insert(found, line);
        }
        Ok(())
    }

    fn check_deadline(&self, child_kind: ProjectKind, parent: Option<&ItemId>) -> Result<(), MoveError> {
        let Some(parent_item) = parent.and_then(|parent_id| self.get(parent_id)) else { return Ok(()) };
        let ProjectKind::Closed { deadline: parent_deadline } = &parent_item.kind else { return Ok(()) };
        let ProjectKind::Closed { deadline: child_deadline } = child_kind else { return Ok(()) };
        if child_deadline > *parent_deadline { Err(MoveError::DeadlineExceeded) } else { Ok(()) }
    }

    fn check_parent(&self, id: &ItemId, parent: Option<&ItemId>) -> Result<(), MoveError> {
        if !self.contains(id) {
            return Err(MoveError::UnknownItem);
        }
        if let Some(parent_id) = parent {
            if !self.contains(parent_id) {
                return Err(MoveError::UnknownParent);
            }
            if parent_id == id {
                return Err(MoveError::SelfParent);
            }
            if path_to_root(self, parent_id.clone()).contains(id) {
                return Err(MoveError::Cycle);
            }
        }
        Ok(())
    }

    pub fn place(&mut self, id: &ItemId, parent: Option<ItemId>, before: Option<ItemId>) -> Result<(), MoveError> {
        self.check_parent(id, parent.as_ref())?;
        self.check_deadline(self.get(id).ok_or(MoveError::UnknownItem)?.kind, parent.as_ref())?;
        if let Some(before_id) = &before {
            let target = self.get(before_id).ok_or(MoveError::UnknownItem)?;
            if target.parent != parent {
                return Err(MoveError::UnknownParent);
            }
            if before_id == id {
                return Ok(());
            }
        }
        let current = self.position(id).ok_or(MoveError::UnknownItem)?;
        let mut line = self.0.remove(current);
        line.parent = parent;
        let slot = before.and_then(|before_id| self.position(&before_id)).unwrap_or(self.0.len());
        self.0.insert(slot, line);
        Ok(())
    }

    pub fn reparent(&mut self, id: &ItemId, parent: Option<ItemId>) -> Result<(), MoveError> {
        self.check_parent(id, parent.as_ref())?;
        self.check_deadline(self.get(id).ok_or(MoveError::UnknownItem)?.kind, parent.as_ref())?;
        let stored = self.0.iter_mut().find(|line| line.id == *id);
        if let Some(line) = stored {
            line.parent = parent;
        }
        Ok(())
    }

    pub fn next_id(&self, random_bytes: &mut impl FnMut() -> u8) -> ItemId {
        let mut entropy: [u8; 3] = array::from_fn(|_| random_bytes());
        loop {
            let id = Mnemonic::from_entropy(&entropy);
            if self.0.iter().all(|line| line.id.0 != id) {
                return ItemId(id);
            }
            for byte in &mut entropy {
                *byte = byte.wrapping_add(1);
            }
        }
    }
}

#[must_use]
fn idle_days(store: &Store, id: &ItemId, today: Date) -> Option<i64> {
    store.last_touched(id).and_then(|touched| today.since(touched).ok()).map(|span| i64::from(span.get_days()))
}

impl<'store> IntoIterator for &'store Store {
    type Item = &'store Item;
    type IntoIter = slice::Iter<'store, Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use jiff::civil;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use crate::{Commitment, Item, ItemId, MoveError, Mnemonic, ProjectKind, Status, Store};

    #[test]
    fn reorder_swaps_within_siblings() {
        let mut store = Store::empty();
        store.add(line("acid-amber-anchor", None, ProjectKind::Open));
        store.add(line("basalt-beech-bell", None, ProjectKind::Open));
        let first = store.0[0].id.clone();
        store.reorder(&first, crate::Step::Falling).unwrap();
        assert_eq!(store.position(&first), Some(1));
    }

    fn line(id: &str, parent: Option<ItemId>, kind: ProjectKind) -> Item {
        Item {
            id: ItemId(Mnemonic::parse(id).unwrap()),
            title: "line".to_owned(),
            parent,
            kind,
            commitment: Commitment::None,
            tags: Vec::new(),
            status: Status::Active,
            notes: String::new(),
            history: Vec::new(),
        }
    }

    #[test]
    fn place_rejects_late_milestone() {
        let mut store = Store::empty();
        store.add(line("acid-amber-anchor", None, ProjectKind::Closed { deadline: civil::date(2030, 1, 1) }));
        store.add(line("basalt-beech-bell", None, ProjectKind::Closed { deadline: civil::date(2030, 6, 1) }));
        let milestone = line("gulf-crest-dune", None, ProjectKind::Closed { deadline: civil::date(2030, 3, 1) });
        store.add(milestone);
        let milestone_id = store.0[2].id.clone();
        assert_eq!(store.place(&milestone_id, Some(store.0[0].id.clone()), None), Err(MoveError::DeadlineExceeded));
        assert_eq!(store.0[2].parent, None);
    }

    #[test]
    fn reparent_allows_fitting_milestone() {
        let mut store = Store::empty();
        store.add(line("acid-amber-anchor", None, ProjectKind::Closed { deadline: civil::date(2030, 6, 1) }));
        store.add(line("basalt-beech-bell", None, ProjectKind::Closed { deadline: civil::date(2030, 3, 1) }));
        let milestone_id = store.0[1].id.clone();
        let project = store.0[0].id.clone();
        assert_eq!(store.place(&milestone_id, Some(project), None), Ok(()));
        assert_eq!(store.0[1].parent.as_ref().map(|parent_id| parent_id.0.to_string()), Some("acid-amber-anchor".to_owned()));
    }

    #[test]
    fn mnemonic_round_trips() {
        let entropy = [42_u8; 16];
        let spelled = Mnemonic::from_entropy(&entropy);
        assert_eq!(Mnemonic::parse(&spelled.to_string()).unwrap(), spelled);
    }

    #[test]
    fn parse_rejects_unknown_word() {
        assert!(Mnemonic::parse("acid banana").is_err());
        assert!(Mnemonic::parse("").is_err());
    }

    #[test]
    fn store_rejects_unknown_parent() {
        let mut store = Store::empty();
        let stray = ItemId(Mnemonic::from_entropy(&[7_u8; 16]));
        let result = catch_unwind(AssertUnwindSafe(|| {
            store.add(line("acid-amber-anchor", Some(stray), ProjectKind::Open));
        }));
        assert!(result.is_err());
    }

    #[test]
    fn days_remaining_counts_down() {
        let mut store = Store::empty();
        store.add(line("acid-amber-anchor", None, ProjectKind::Closed { deadline: civil::date(2030, 12, 31) }));
        let today = civil::date(2030, 12, 30);
        let line = store.get(&ItemId(Mnemonic::parse("acid-amber-anchor").unwrap())).unwrap();
        assert_eq!(crate::days_remaining(line, today), Some(1));
    }

    #[test]
    fn path_to_root_walks_nesting() {
        let mut store = Store::empty();
        let root = "acid-amber-anchor";
        store.add(line(root, None, ProjectKind::Open));
        store.add(line("basalt-beech-bell", Some(ItemId(Mnemonic::parse(root).unwrap())), ProjectKind::Open));
        let path = crate::path_to_root(&store, ItemId(Mnemonic::parse("basalt-beech-bell").unwrap()));
        assert_eq!(path.len(), 2);
        assert_eq!(path[1].0.to_string(), root);
    }

    #[test]
    fn reorder_skips_foreign_siblings() {
        let mut store = Store::empty();
        store.add(line("acid-amber-anchor", None, ProjectKind::Open));
        store.add(line("basalt-beech-bell", Some(ItemId(Mnemonic::parse("acid-amber-anchor").unwrap())), ProjectKind::Open));
        store.add(line("gulf-arrow-fig", None, ProjectKind::Open));
        let last = store.0[2].id.clone();
        store.reorder(&last, crate::Step::Rising).unwrap();
        assert_eq!(store.position(&last), Some(0));
    }

    #[test]
    fn reparent_denies_cycle() {
        let mut store = Store::empty();
        store.add(line("acid-amber-anchor", None, ProjectKind::Open));
        store.add(line("basalt-beech-bell", None, ProjectKind::Open));
        let root = store.0[0].id.clone();
        let child = store.0[1].id.clone();
        store.reparent(&root, Some(child.clone())).unwrap();
        assert_eq!(store.reparent(&child, Some(root.clone())), Err(MoveError::Cycle));
    }

    #[test]
    fn proposals_confirm_into_active() {
        let mut store = Store::empty();
        store.add(line("acid-amber-anchor", None, ProjectKind::Open));
        let id = store.0[0].id.clone();
        store.0[0].status = Status::Proposed;
        assert_eq!(store.confirm(&id, civil::date(2031, 1, 1)), Ok(()));
        assert_eq!(store.0[0].status, Status::Active);
        assert_eq!(store.confirm(&id, civil::date(2031, 1, 2)), Err(MoveError::UnknownItem));
    }

    #[test]
    fn skips_decay_after_three() {
        let mut store = Store::empty();
        store.add(line("acid-amber-anchor", None, ProjectKind::Open));
        let id = store.0[0].id.clone();
        for day in 1..3 {
            store.skip(&id, civil::date(2031, 1, day)).unwrap();
        }
        assert!(!store.decayed(&id, civil::date(2031, 1, 5)));
        store.skip(&id, civil::date(2031, 1, 6)).unwrap();
        assert!(store.decayed(&id, civil::date(2031, 1, 6)));
    }

    #[test]
    fn idle_items_decay_after_fourteen_days() {
        let mut store = Store::empty();
        store.add(line("acid-amber-anchor", None, ProjectKind::Open));
        let id = store.0[0].id.clone();
        store.0[0].history.push(crate::Event { on: civil::date(2031, 1, 1), kind: crate::EventKind::Captured, detail: String::new() });
        assert!(!store.decayed(&id, civil::date(2031, 1, 14)));
        assert!(store.decayed(&id, civil::date(2031, 1, 15)));
    }

    #[test]
    fn reason_text_reports_deadline_and_skips() {
        let mut store = Store::empty();
        store.add(line("acid-amber-anchor", None, ProjectKind::Closed { deadline: civil::date(2031, 1, 10) }));
        let id = store.0[0].id.clone();
        assert_eq!(store.reason_text(&id, civil::date(2031, 1, 8)), "due in 2 days");
        assert_eq!(store.reason_text(&id, civil::date(2031, 1, 12)), "2 days overdue");
        store.skip(&id, civil::date(2031, 1, 8)).unwrap();
        store.skip(&id, civil::date(2031, 1, 8)).unwrap();
        store.skip(&id, civil::date(2031, 1, 8)).unwrap();
        assert_eq!(store.reason_text(&id, civil::date(2031, 1, 8)), "due in 2 days · skipped 3 times");
    }

    #[test]
    fn next_id_avoids_collision() {
        let mut store = Store::empty();
        let mut counter = 0_u8;
        store.0.push(line("acid-amber-anchor", None, ProjectKind::Open));
        let fresh = store.next_id(&mut || {
            counter += 1;
            counter.wrapping_mul(37)
        });
        assert!(!store.contains(&fresh));
    }
}
