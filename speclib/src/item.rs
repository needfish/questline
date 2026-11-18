use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::Mnemonic;

pub use jiff::civil::Date;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemId(pub Mnemonic);

impl Display for ItemId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.0)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: ItemId,
    pub title: String,
    pub parent: Option<ItemId>,
    pub kind: ProjectKind,
    pub commitment: Commitment,
    pub tags: Vec<String>,
    pub status: Status,
    #[serde(default)]
    pub notes: String,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum ProjectKind {
    Open,
    Closed { deadline: Date },
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum Commitment {
    None,
    Daily { last_done: Option<Date> },
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Active,
    Done,
    Archived,
}

#[derive(Serialize, Deserialize)]
pub struct Store(pub Vec<Item>);

#[derive(Clone, Copy, Debug)]
pub enum Direction {
    Rising,
    Falling,
}

#[must_use]
pub const fn count_direction(line: &Item) -> Direction {
    match line.kind {
        ProjectKind::Open => Direction::Rising,
        ProjectKind::Closed { .. } => Direction::Falling,
    }
}

#[must_use]
pub fn days_remaining(line: &Item, today: Date) -> Option<i64> {
    match line.kind {
        ProjectKind::Closed { deadline } => deadline.since(today).ok().map(|span| i64::from(span.get_days())),
        ProjectKind::Open => None,
    }
}

#[must_use]
pub const fn commitment_kind(line: &Item) -> Option<Direction> {
    match line.commitment {
        Commitment::Daily { .. } => Some(Direction::Rising),
        Commitment::None => None,
    }
}

#[must_use]
pub fn path_to_root(store: &Store, start: ItemId) -> Vec<ItemId> {
    let mut path = Vec::new();
    let mut current = Some(start);
    while let Some(current_id) = current {
        assert!(path.len() < 1000, "cycle in item ancestry");
        path.push(current_id.clone());
        current = store.get(&current_id).and_then(|line| line.parent.clone());
    }
    path
}
