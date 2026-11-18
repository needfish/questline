use speclib::{Commitment, Event, EventKind, Item, ItemId, MoveError, Mnemonic, MnemonicError, ProjectKind, Status, Store};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use jiff::civil::Date;

use tiny_http::{Method, Request, Response};

use crate::extract;
use crate::static_files::serve;
use crate::store_file::{save, ServerError, ServerResult};

use ServerError::Json;

#[derive(Debug, Default, serde::Deserialize)]
struct MoveInstruction {
    #[serde(default)]
    rise: bool,
    #[serde(default)]
    fall: bool,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    before: Option<String>,
}

pub(crate) fn handle(request: Request, store: &mut Store, store_path: &str) -> ServerResult<()> {
    let method = request.method().clone();
    let url = request.url().to_owned();
    if method == Method::Get && url == "/items" {
        list(request, store)
    } else if method == Method::Get && url.starts_with("/reason") {
        reasons(request, store, &url)
    } else if method == Method::Post && url == "/items" {
        create(request, store, store_path)
    } else if method == Method::Post && url == "/capture" {
        capture(request, store, store_path)
    } else if method == Method::Post && url.starts_with("/items/") && url.ends_with("/move") {
        reindex(request, store, store_path, &url)
    } else if method == Method::Post && url.starts_with("/items/") && url.ends_with("/deadline") {
        retime(request, store, store_path, &url)
    } else if method == Method::Post && url.starts_with("/items/") && url.ends_with("/notes") {
        renote(request, store, store_path, &url)
    } else if method == Method::Post && url.starts_with("/items/") && url.ends_with("/confirm") {
        settle(request, store, store_path, &url, Settlement::Confirm)
    } else if method == Method::Post && url.starts_with("/items/") && url.ends_with("/skip") {
        settle(request, store, store_path, &url, Settlement::Skip)
    } else if method == Method::Post && url.starts_with("/items/") && url.ends_with("/drop") {
        settle(request, store, store_path, &url, Settlement::Drop)
    } else if method == Method::Post && url.starts_with("/items/") {
        mark_done(request, store, store_path, &url)
    } else if method == Method::Get {
        serve(request, &url)
    } else {
        respond(request, 404, "not found")
    }
}

fn list(request: Request, store: &Store) -> ServerResult<()> {
    respond_raw(request, 200, &serde_json::to_string(store)?)
}

fn create(mut request: Request, store: &mut Store, store_path: &str) -> ServerResult<()> {
    let body = drain(&mut request)?;
    if body.is_empty() {
        return respond(request, 422, "empty request body");
    }
    let draft: ItemDraft = serde_json::from_str(&body)?;
    let mut random_bytes = lcg(seed());
    let id = store.next_id(&mut random_bytes);
    let Ok(built) = draft.build(id) else {
        return respond(request, 422, "bad parent id");
    };
    if built.parent.as_ref().is_some_and(|parent_id| !store.contains(parent_id)) {
        return respond(request, 404, "unknown parent");
    }
    let id_text = built.id.to_string();
    store.add(built);
    save(store, store_path)?;
    respond(request, 201, &id_text)
}

#[derive(serde::Deserialize)]
struct CaptureDraft {
    text: String,
    #[serde(default)]
    parent: Option<String>,
    today: Date,
}

fn capture(mut request: Request, store: &mut Store, store_path: &str) -> ServerResult<()> {
    let body = drain(&mut request)?;
    let draft: CaptureDraft = serde_json::from_str(&body)?;
    let kind = match extract::classify(&draft.text, store) {
        extract::Classification::Question => {
            let message = extract::answer(&draft.text, store, draft.today);
            return respond_raw(request, 200, &serde_json::to_string(&CaptureOutcome { kind: "question", message })?);
        }
        extract::Classification::Correction(target_id) => {
            let message = apply_correction(store, &target_id, &draft.text, draft.today);
            save(store, store_path)?;
            return respond_raw(request, 200, &serde_json::to_string(&CaptureOutcome { kind: "correction", message })?);
        }
        extract::Classification::Scratch => "scratch",
        extract::Classification::Capture => "capture",
    };
    let parent = draft.parent.as_ref().map(|text| Mnemonic::parse(text).map(ItemId)).transpose()?;
    if parent.as_ref().is_some_and(|parent_id| !store.contains(parent_id)) {
        return respond(request, 404, "unknown parent");
    }
    let text = if kind == "scratch" { draft.text.trim_start_matches('-').trim() } else { draft.text.trim() };
    let hit = extract::parse_deadline(text, draft.today);
    let (title, parent) = split_slot(text, parent, store, kind == "capture");
    let proposal = build_proposal(store, &title, parent, hit.as_ref(), draft.today, kind)?;
    let title = proposal.title.clone();
    store.add(proposal);
    save(store, store_path)?;
    let message = format!("proposed: {title}");
    respond_raw(request, 201, &serde_json::to_string(&CaptureOutcome { kind, message })?)
}

#[derive(serde::Serialize)]
struct CaptureOutcome {
    kind: &'static str,
    message: String,
}

fn split_slot(text: &str, client_parent: Option<ItemId>, store: &Store, slot: bool) -> (String, Option<ItemId>) {
    let auto = client_parent.is_none() && slot;
    if auto {
        if let Some((descriptor, subject)) = text.split_once(',') {
            let title = extract::title_case(subject);
            if !title.is_empty() {
                if let Some(area) = extract::find_area(descriptor, store) {
                    return (title, Some(area));
                }
            }
        }
    }
    (extract::format_title(text), client_parent)
}

fn build_proposal(store: &Store, title: &str, parent: Option<ItemId>, hit: Option<&extract::DeadlineHit>, today: Date, kind: &str) -> ServerResult<Item> {
    let mut random_bytes = lcg(seed());
    let id = store.next_id(&mut random_bytes);
    let phrase = hit.map_or("no date", |found| found.phrase.trim());
    if title.is_empty() {
        return Err(Json(serde::de::Error::custom("empty title")));
    }
    Ok(Item {
        id,
        title: title.to_owned(),
        parent,
        kind: hit.map_or(ProjectKind::Open, |found| ProjectKind::Closed { deadline: found.at }),
        commitment: Commitment::None,
        tags: Vec::new(),
        status: Status::Proposed,
        notes: String::new(),
        history: proposal_history(today, kind, phrase),
    })
}

fn strip_phrase(text: &str, phrase: &str) -> String {
    let lowered = text.to_lowercase();
    let Some(start) = lowered.find(&phrase.to_lowercase()) else {
        return text.trim().to_owned();
    };
    let end = start.checked_add(phrase.len()).unwrap_or(text.len());
    let lead = text.get(..start).unwrap_or_default();
    let tail = text.get(end..).unwrap_or_default();
    format!("{lead} {tail}").split_whitespace().collect::<Vec<&str>>().join(" ")
}

fn apply_correction(store: &mut Store, id: &ItemId, text: &str, today: Date) -> String {
    let lowered = text.to_lowercase();
    let drops = ["drop", "forget", "cancel", "never mind"].iter().any(|cue| lowered.contains(cue));
    if drops {
        if store.drop_item(id, today).is_ok() {
            return "dropped".to_owned();
        }
    }
    let pushes = ["push", "postpone", "defer", "move"].iter().any(|cue| lowered.contains(cue));
    if pushes {
        if let Some(hit) = extract::parse_deadline(text, today) {
            if store.set_deadline(id, hit.at).is_ok() {
                return format!("moved to {}", hit.phrase.trim());
            }
        }
    }
    let existing = store.get(id).map_or(String::new(), |line| line.notes.clone());
    let joined = if existing.is_empty() { text.trim().to_owned() } else { format!("{existing}\n{}", text.trim()) };
    if store.set_notes(id, joined).is_ok() {
        return "noted".to_owned();
    }
    "unknown item".to_owned()
}

fn captured_history(today: Date, detail: &str) -> Vec<Event> {
    let mut events = Vec::new();
    events.push(Event { on: today, kind: EventKind::Captured, detail: detail.to_owned() });
    events
}

fn proposal_history(today: Date, kind: &str, phrase: &str) -> Vec<Event> {
    let mut events = captured_history(today, kind);
    events.push(Event { on: today, kind: EventKind::Inferred, detail: phrase.to_owned() });
    events
}

fn seed() -> u64 {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX)
}

fn mark_done(mut request: Request, store: &mut Store, store_path: &str, url: &str) -> ServerResult<()> {
    let body = drain(&mut request)?;
    let Ok(mnemonic) = Mnemonic::parse(&percent_decode(url.trim_start_matches("/items/"))) else {
        return respond(request, 404, "unknown item");
    };
    let today = parse_today(&body)?;
    let id = ItemId(mnemonic);
    if store.toggle(&id, today).is_err() {
        return respond(request, 404, "not found");
    }
    save(store, store_path)?;
    respond(request, 200, &id.to_string())
}

#[derive(Clone, Copy)]
enum Settlement {
    Confirm,
    Skip,
    Drop,
}

fn settle(mut request: Request, store: &mut Store, store_path: &str, url: &str, settlement: Settlement) -> ServerResult<()> {
    let body = drain(&mut request)?;
    let today = parse_today(&body)?;
    let bare = url.split('/').next_back().unwrap_or_default();
    let id_text = url.trim_start_matches("/items/").trim_end_matches(&format!("/{bare}"));
    let Ok(mnemonic) = Mnemonic::parse(&percent_decode(id_text)) else {
        return respond(request, 404, "unknown item");
    };
    let id = ItemId(mnemonic);
    let applied = match settlement {
        Settlement::Confirm => store.confirm(&id, today),
        Settlement::Skip => store.skip(&id, today),
        Settlement::Drop => store.drop_item(&id, today),
    };
    if applied.is_err() {
        return respond(request, 404, "unknown item");
    }
    save(store, store_path)?;
    respond(request, 200, &id.to_string())
}

fn parse_today(body: &str) -> ServerResult<Date> {
    let parsed = serde_json::from_str::<TodayDraft>(body);
    match parsed {
        Ok(draft) => Ok(draft.today),
        Err(_) => Err(Json(serde::de::Error::custom("missing today"))),
    }
}

#[derive(serde::Deserialize)]
struct TodayDraft {
    today: Date,
}

fn reasons(request: Request, store: &Store, url: &str) -> ServerResult<()> {
    let today = url.rsplit("today=").next().and_then(|text| text.parse::<Date>().ok());
    let Some(today) = today else {
        return respond(request, 422, "missing today");
    };
    let mut mapped = BTreeMap::new();
    for line in store.iter() {
        let text = store.reason_text(&line.id, today);
        if !text.is_empty() {
            mapped.insert(line.id.to_string(), text);
        }
    }
    respond_raw(request, 200, &serde_json::to_string(&mapped)?)
}

fn renote(mut request: Request, store: &mut Store, store_path: &str, url: &str) -> ServerResult<()> {
    let body = drain(&mut request)?;
    let id_text = url.trim_start_matches("/items/").trim_end_matches("/notes");
    let Ok(mnemonic) = Mnemonic::parse(&percent_decode(id_text)) else {
        return respond(request, 404, "unknown item");
    };
    let Ok(text) = serde_json::from_str::<String>(&body) else {
        return respond(request, 422, "bad notes");
    };
    if store.set_notes(&ItemId(mnemonic), text).is_err() {
        return respond(request, 404, "unknown item");
    }
    save(store, store_path)?;
    respond(request, 200, "saved")
}

fn retime(mut request: Request, store: &mut Store, store_path: &str, url: &str) -> ServerResult<()> {
    let body = drain(&mut request)?;
    let id_text = url.trim_start_matches("/items/").trim_end_matches("/deadline");
    let Ok(mnemonic) = Mnemonic::parse(&percent_decode(id_text)) else {
        return respond(request, 404, "unknown item");
    };
    let Ok(deadline) = serde_json::from_str::<Date>(&body) else {
        return respond(request, 422, "bad deadline");
    };
    if store.set_deadline(&ItemId(mnemonic), deadline).is_err() {
        return respond(request, 409, "deadline rejected");
    }
    save(store, store_path)?;
    respond(request, 200, &deadline.to_string())
}

fn reindex(mut request: Request, store: &mut Store, store_path: &str, url: &str) -> ServerResult<()> {
    let id_text = url.trim_start_matches("/items/").trim_end_matches("/move");
    let Ok(mnemonic) = Mnemonic::parse(&percent_decode(id_text)) else {
        return respond(request, 404, "unknown item");
    };
    let id = ItemId(mnemonic);
    let instruction = read_instruction(&mut request)?;
    let Some((parent_id, before_id)) = parse_ids(&instruction) else {
        return respond(request, 404, "unknown item");
    };
    let applied = apply_move(store, &id, &instruction, parent_id, before_id);
    let Err(rejection) = applied else {
        save(store, store_path)?;
        return respond_raw(request, 200, &serde_json::to_string(store)?);
    };
    respond(request, 409, rejection_reason(rejection))
}

fn parse_ids(instruction: &MoveInstruction) -> Option<(Option<ItemId>, Option<ItemId>)> {
    Some((parse_optional(&instruction.parent)?, parse_optional(&instruction.before)?))
}

fn parse_optional(text: &Option<String>) -> Option<Option<ItemId>> {
    let Some(text) = text else { return Some(None) };
    Some(Mnemonic::parse(text).ok().map(ItemId))
}

fn apply_move(
    store: &mut Store,
    id: &ItemId,
    instruction: &MoveInstruction,
    parent_id: Option<ItemId>,
    before_id: Option<ItemId>,
) -> Result<(), MoveError> {
    if instruction.rise {
        reorder(store, id, speclib::Step::Rising)
    } else if instruction.fall {
        reorder(store, id, speclib::Step::Falling)
    } else if instruction.before.is_some() {
        place(store, id, parent_id, before_id)
    } else {
        reparent(store, id, parent_id)
    }
}

fn read_instruction(request: &mut Request) -> ServerResult<MoveInstruction> {
    let body = drain(request)?;
    if body.is_empty() {
        return Ok(MoveInstruction::default());
    }
    Ok(serde_json::from_str(&body)?)
}

fn drain(request: &mut Request) -> ServerResult<String> {
    let mut body = String::new();
    let _ = request.as_reader().read_to_string(&mut body)?;
    Ok(body)
}

fn decode_pair(pair: [u8; 2]) -> Option<u8> {
    u8::from_str_radix(str::from_utf8(&pair).ok()?, 16).ok()
}

fn percent_decode(text: &str) -> String {
    let mut decoded = String::new();
    let mut bytes = text.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        let escape = if byte == b'%' {
            let pair = bytes.next().and_then(|first| bytes.next().map(|second| [first, second]));
            pair.and_then(decode_pair)
        } else {
            None
        };
        decoded.push(char::from(escape.unwrap_or(byte)));
    }
    decoded
}

fn reparent(store: &mut Store, id: &ItemId, parent: Option<ItemId>) -> Result<(), MoveError> {
    store.reparent(id, parent)
}

fn place(store: &mut Store, id: &ItemId, parent: Option<ItemId>, before: Option<ItemId>) -> Result<(), MoveError> {
    store.place(id, parent, before)
}

fn reorder(store: &mut Store, id: &ItemId, step: speclib::Step) -> Result<(), MoveError> {
    store.reorder(id, step)
}

const fn rejection_reason(rejection: MoveError) -> &'static str {
    match rejection {
        MoveError::UnknownItem | MoveError::UnknownParent => "unknown item",
        MoveError::SelfParent => "item cannot parent itself",
        MoveError::Cycle => "move would create a cycle",
        MoveError::DeadlineExceeded => "milestone deadline later than parent deadline",
    }
}

fn lcg(seed: u64) -> impl FnMut() -> u8 {
    let mut state = seed;
    move || {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        u8::try_from(state >> 56).unwrap_or_default()
    }
}

fn respond(request: Request, code: u16, text: &str) -> ServerResult<()> {
    respond_raw(request, code, &serde_json::to_string(&text)?)
}

fn respond_raw(request: Request, code: u16, body: &str) -> ServerResult<()> {
    let Ok(header) = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]) else {
        return Err(ServerError::Header);
    };
    request.respond(Response::from_string(body).with_status_code(code).with_header(header))?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct ItemDraft {
    title: String,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    deadline: Option<Date>,
    #[serde(default)]
    today: Option<Date>,
}

impl ItemDraft {
    fn build(self, id: ItemId) -> Result<Item, MnemonicError> {
        let parent = match self.parent {
            Some(text) => Some(ItemId(Mnemonic::parse(&text)?)),
            None => None,
        };
        let kind = self.deadline.map_or(ProjectKind::Open, |deadline| ProjectKind::Closed { deadline });
        let history = self.today.map_or_else(Vec::new, |today| captured_history(today, "form"));
        Ok(Item {
            id,
            title: self.title,
            parent,
            kind,
            commitment: Commitment::None,
            tags: Vec::new(),
            status: Status::Active,
            notes: String::new(),
            history,
        })
    }
}

