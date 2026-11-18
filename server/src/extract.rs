use jiff::civil::Date;
use speclib::{days_remaining, Item, ItemId, ProjectKind, Status, Store};

pub struct DeadlineHit {
    pub at: Date,
    pub phrase: String,
}

pub enum Classification {
    Capture,
    Scratch,
    Question,
    Correction(ItemId),
}

const WEEKDAYS: [&str; 7] = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"];
const MONTHS: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];
const CONNECTORS: [&str; 6] = ["by", "before", "on", "due", "until", "around"];
const STOP: [&str; 8] = ["the", "and", "for", "with", "from", "that", "this", "into"];

pub fn classify(text: &str, store: &Store) -> Classification {
    let trimmed = text.trim();
    if trimmed.ends_with('?') {
        return Classification::Question;
    }
    if trimmed.starts_with('-') {
        return Classification::Scratch;
    }
    if correction_cue(trimmed) {
        if let Some(target) = find_target(trimmed, store) {
            return Classification::Correction(target);
        }
    }
    Classification::Capture
}

fn correction_cue(text: &str) -> bool {
    ["drop", "forget", "cancel", "push", "postpone", "defer", "move", "no rush", "can wait"]
        .iter().any(|cue| text.to_lowercase().contains(cue))
}

fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '\'')
        .filter(|word| word.len() > 2 && !STOP.contains(word))
        .map(ToOwned::to_owned)
        .collect()
}

fn find_target(text: &str, store: &Store) -> Option<ItemId> {
    let words = tokens(text);
    let mut best: Option<(usize, ItemId)> = None;
    for line in store.iter() {
        if line.status != Status::Active && line.status != Status::Proposed {
            continue;
        }
        let title = tokens(&line.title);
        let hits = words.iter().filter(|word| title.contains(*word)).count();
        let doubled = hits.checked_mul(2).unwrap_or_default();
        let strong = hits >= 2 || (hits > 0 && doubled >= title.len());
        if strong && best.as_ref().map_or(true, |(score, _)| hits > *score) {
            best = Some((hits, line.id.clone()));
        }
    }
    best.map(|(_, id)| id)
}

pub fn answer(question: &str, store: &Store, today: Date) -> String {
    let lowered = question.to_lowercase();
    let overdue: Vec<&Item> = store.iter()
        .filter(|line| line.status == Status::Active && days_remaining(line, today).map_or(false, |days| days < 0))
        .collect();
    if lowered.contains("overdue") || lowered.contains("late") {
        return if overdue.is_empty() {
            "nothing overdue".to_owned()
        } else {
            format!("overdue: {}", titles(&overdue))
        };
    }
    if lowered.contains("due") || lowered.contains("deadline") || lowered.contains("soon") {
        let due: Vec<&Item> = store.iter()
            .filter(|line| line.status == Status::Active && days_remaining(line, today).map_or(false, |days| days >= 0))
            .collect();
        return if due.is_empty() {
            "nothing due".to_owned()
        } else {
            format!("due: {}", titles(&due))
        };
    }
    let active = store.iter().filter(|line| line.status == Status::Active).count();
    format!("{active} active lines. ask overdue or due.")
}

const SMALL: [&str; 6] = ["a", "an", "the", "of", "in", "on"];

pub fn find_area(text: &str, store: &Store) -> Option<ItemId> {
    let words = tokens(text);
    let mut best: Option<(usize, ItemId)> = None;
    for line in store.iter() {
        if line.parent.is_some() || line.kind != ProjectKind::Open || line.status != Status::Active {
            continue;
        }
        let area = tokens(&line.title);
        let hits = words.iter().filter(|word| area.contains(*word)).count();
        if hits > 0 && best.as_ref().map_or(true, |(score, _)| hits > *score) {
            best = Some((hits, line.id.clone()));
        }
    }
    best.map(|(_, id)| id)
}

pub fn format_title(text: &str) -> String {
    let trimmed = text.trim().trim_end_matches(|ch: char| matches!(ch, '.' | '!' | '?'));
    let mut chars = trimmed.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub fn title_case(text: &str) -> String {
    text.split_whitespace()
        .enumerate()
        .map(|(index, word)| {
            let lower = word.to_lowercase();
            let raised = index > 0 && SMALL.contains(&lower.as_str());
            if raised {
                lower
            } else {
                let mut chars = lower.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => lower,
                }
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

fn titles(lines: &[&Item]) -> String {
    lines.iter().map(|line| line.title.as_str()).collect::<Vec<&str>>().join(", ")
}

fn word_list(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut rest = text.trim_start();
    let mut offset = 0_usize;
    while let Some(space) = rest.find(' ') {
        let (word, remainder) = rest.split_at(space);
        if !word.is_empty() {
            out.push((offset, word.to_lowercase()));
        }
        let trimmed = remainder.trim_start_matches(' ');
        offset = offset
            .saturating_add(space)
            .saturating_add(remainder.len().saturating_sub(trimmed.len()));
        rest = trimmed;
    }
    if !rest.is_empty() {
        out.push((offset, rest.to_lowercase()));
    }
    out
}

fn plain(word: &str) -> &str {
    word.trim_end_matches(|ch: char| ch == ',' || ch == '.' || ch == '!')
}

pub fn parse_deadline(text: &str, today: Date) -> Option<DeadlineHit> {
    let words = word_list(text);
    let mut best: Option<DeadlineHit> = None;
    for (index, (start, word)) in words.iter().enumerate() {
        let next = words.get(index.checked_add(1)?).map(|(_, text)| plain(text));
        let next_two = words.get(index.checked_add(2)?).map(|(_, text)| plain(text));
        let previous = index.checked_sub(1).and_then(|back| words.get(back)).map(|(_, text)| text.as_str());
        let (at, span) = match (plain(word), next, next_two) {
            ("today", _, _) => (Some(today), 1_usize),
            ("tomorrow", _, _) => (today.tomorrow().ok(), 1),
            ("in", Some(days), Some(unit)) if unit.starts_with("day") => {
                (days.parse::<i64>().ok().and_then(|count| checked_day(today, count)), 3)
            }
            (word, _, _) if WEEKDAYS.contains(&word) => (Some(next_weekday(today, word, previous == Some("next"))), 1),
            (word, Some(day), _) if MONTHS.contains(&word) => (month_day(word, day, next_two, today), 2),
            (day, Some(month), _) if day.parse::<u8>().is_ok() && MONTHS.contains(&month) => {
                (month_day(month, day, next_two, today), 2)
            }
            (word, _, _) if is_iso(word) => (word.parse::<Date>().ok(), 1),
            _ => (None, 0),
        };
        if let Some(at) = at {
            let last = index.checked_add(span.saturating_sub(1)).and_then(|tip| words.get(tip));
            let end = last.map_or(*start, |(offset, text)| offset.saturating_add(text.len()));
            let phrase = extend_connector(text, *start, end);
            best = Some(DeadlineHit { at, phrase });
            break;
        }
    }
    best
}

fn extend_connector(text: &str, start: usize, end: usize) -> String {
    let before = text.get(..start).unwrap_or_default().trim_end();
    let last = before.rsplit(' ').next().unwrap_or_default().to_lowercase();
    let phrase_start = if CONNECTORS.contains(&last.as_str()) {
        start.saturating_sub(last.len().saturating_add(1))
    } else {
        start
    };
    text.get(phrase_start..end).unwrap_or_default().to_owned()
}

fn is_iso(word: &str) -> bool {
    let parts: Vec<&str> = word.split('-').collect();
    parts.len() == 3
        && parts.iter().all(|part| part.len() == 2 || part.len() == 4)
        && word.parse::<Date>().is_ok()
}

fn checked_day(today: Date, count: i64) -> Option<Date> {
    if count < 0 {
        return None;
    }
    today.checked_add(jiff::Span::new().days(count)).ok()
}

fn next_weekday(today: Date, word: &str, force_next: bool) -> Date {
    let Some(wanted) = WEEKDAYS.iter().position(|name| *name == word) else {
        return today;
    };
    let mut cursor = if force_next { today.tomorrow().unwrap_or(today) } else { today };
    while i16::from(cursor.weekday().to_monday_zero_offset()) != i16::try_from(wanted).unwrap_or_default() {
        cursor = cursor.tomorrow().unwrap_or(cursor);
    }
    cursor
}

fn month_day(month: &str, day: &str, year: Option<&str>, today: Date) -> Option<Date> {
    let month_index = i8::try_from(MONTHS.iter().position(|name| *name == month)?.checked_add(1)?).unwrap_or_default();
    let day_number = day.parse::<i8>().ok()?;
    let year_number = year.and_then(|text| text.parse::<i16>().ok()).unwrap_or_else(|| {
        let this_year = Date::new(today.year(), month_index, day_number).ok();
        let past = this_year.map_or(true, |date| date.since(today).map(|span| span.get_days()).unwrap_or(0) < 0);
        if past { i16::try_from(today.year()).ok().and_then(|year| year.checked_add(1)).unwrap_or(today.year()) } else { today.year() }
    });
    Date::new(year_number, month_index, day_number).ok()
}

#[cfg(test)]
mod tests {
    use super::{answer, parse_deadline};
    use jiff::civil;

    #[test]
    fn parses_iso_and_phrases() {
        let today = civil::date(2031, 3, 10);
        let hit = parse_deadline("send the draft by 2031-04-01", today).unwrap();
        assert_eq!(hit.at, civil::date(2031, 4, 1));
        assert_eq!(hit.phrase, "by 2031-04-01");
        let hit = parse_deadline("call Maya tomorrow", today).unwrap();
        assert_eq!(hit.at, civil::date(2031, 3, 11));
        let hit = parse_deadline("file taxes in 3 days", today).unwrap();
        assert_eq!(hit.at, civil::date(2031, 3, 13));
        let hit = parse_deadline("submit by friday", today).unwrap();
        assert_eq!(hit.at, civil::date(2031, 3, 14));
        assert_eq!(hit.phrase, "by friday");
        assert!(parse_deadline("water the plants", today).is_none());
    }

    #[test]
    fn month_dates_roll_to_next_year() {
        let today = civil::date(2031, 3, 10);
        let hit = parse_deadline("renew passport by april 5", today).unwrap();
        assert_eq!(hit.at, civil::date(2031, 4, 5));
        let hit = parse_deadline("renew passport by january 5", today).unwrap();
        assert_eq!(hit.at, civil::date(2032, 1, 5));
    }

    #[test]
    fn formats_and_slots_comma_captures() {
        let today = civil::date(2031, 3, 10);
        let mut store = speclib::Store::empty();
        store.add(speclib::Item {
            id: speclib::ItemId(speclib::Mnemonic::parse("acid amber anchor").unwrap()),
            title: "Reading practice".to_owned(),
            parent: None,
            kind: speclib::ProjectKind::Open,
            commitment: speclib::Commitment::None,
            tags: Vec::new(),
            status: speclib::Status::Active,
            notes: String::new(),
            history: Vec::new(),
        });
        assert_eq!(super::title_case("moby dick, the return"), "Moby Dick, the Return");
        assert_eq!(super::format_title("water the plants!!"), "Water the plants");
        assert_eq!(
            super::find_area("new reading", &store),
            Some(speclib::ItemId(speclib::Mnemonic::parse("acid amber anchor").unwrap()))
        );
        assert_eq!(super::find_area("groceries", &store), None);
    }

    #[test]
    fn answers_overdue_and_due() {
        let today = civil::date(2031, 3, 10);
        let mut store = speclib::Store::empty();
        store.add(speclib::Item {
            id: speclib::ItemId(speclib::Mnemonic::parse("acid amber anchor").unwrap()),
            title: "old task".to_owned(),
            parent: None,
            kind: speclib::ProjectKind::Closed { deadline: civil::date(2031, 3, 1) },
            commitment: speclib::Commitment::None,
            tags: Vec::new(),
            status: speclib::Status::Active,
            notes: String::new(),
            history: Vec::new(),
        });
        assert_eq!(answer("what is overdue?", &store, today), "overdue: old task");
        assert_eq!(answer("anything due?", &store, today), "nothing due");
    }
}
