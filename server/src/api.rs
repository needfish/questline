use speclib::{Commitment, Item, ItemId, MoveError, Mnemonic, MnemonicError, ProjectKind, Status, Store};
use std::time::{SystemTime, UNIX_EPOCH};

use jiff::civil::{self, Date};

use tiny_http::{Method, Request, Response};

use crate::static_files::serve;
use crate::store_file::{save, ServerError, ServerResult};

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
    } else if method == Method::Post && url == "/items" {
        create(request, store, store_path)
    } else if method == Method::Post && url.starts_with("/items/") && url.ends_with("/move") {
        reindex(request, store, store_path, &url)
    } else if method == Method::Post && url.starts_with("/items/") && url.ends_with("/deadline") {
        retime(request, store, store_path, &url)
    } else if method == Method::Post && url.starts_with("/items/") && url.ends_with("/notes") {
        renote(request, store, store_path, &url)
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
    let mut body = String::new();
    let byte_count = request.as_reader().read_to_string(&mut body)?;
    if byte_count == 0 {
        return respond(request, 422, "empty request body");
    }
    let draft: ItemDraft = serde_json::from_str(&body)?;
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let seed = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
    let mut random_bytes = lcg(seed);
    let id = store.next_id(&mut random_bytes);
    let built = draft.build(id)?;
    let id_text = built.id.to_string();
    store.add(built);
    save(store, store_path)?;
    respond(request, 201, &id_text)
}

fn mark_done(request: Request, store: &mut Store, store_path: &str, url: &str) -> ServerResult<()> {
    let Ok(mnemonic) = Mnemonic::parse(&percent_decode(url.trim_start_matches("/items/"))) else {
        return respond(request, 404, "unknown item");
    };
    let id = ItemId(mnemonic);
    if store.toggle(&id).is_err() {
        return respond(request, 404, "not found");
    }
    save(store, store_path)?;
    respond(request, 200, &id.to_string())
}

fn renote(mut request: Request, store: &mut Store, store_path: &str, url: &str) -> ServerResult<()> {
    let id_text = url.trim_start_matches("/items/").trim_end_matches("/notes");
    let Ok(mnemonic) = Mnemonic::parse(&percent_decode(id_text)) else {
        return respond(request, 404, "unknown item");
    };
    let mut body = String::new();
    let _ = request.as_reader().read_to_string(&mut body)?;
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
    let id_text = url.trim_start_matches("/items/").trim_end_matches("/deadline");
    let Ok(mnemonic) = Mnemonic::parse(&percent_decode(id_text)) else {
        return respond(request, 404, "unknown item");
    };
    let mut body = String::new();
    let _ = request.as_reader().read_to_string(&mut body)?;
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
    let mut body = String::new();
    let byte_count = request.as_reader().read_to_string(&mut body)?;
    if byte_count == 0 {
        return Ok(MoveInstruction::default());
    }
    Ok(serde_json::from_str(&body)?)
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

fn rejection_reason(rejection: MoveError) -> &'static str {
    match rejection {
        MoveError::UnknownItem | MoveError::UnknownParent => "unknown item",
        MoveError::SelfParent => "item cannot parent itself",
        MoveError::Cycle => "move would create a cycle",
        MoveError::DeadlineExceeded => "milestone deadline later than parent deadline",
        MoveError::NotClosed => "item has no deadline",
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

struct ItemDraft {
    title: String,
    parent: Option<String>,
    open: bool,
    deadline: Option<Date>,
}

impl<'de> serde::Deserialize<'de> for ItemDraft {
    fn deserialize<Des: serde::Deserializer<'de>>(deserializer: Des) -> Result<Self, Des::Error> {
        #[derive(serde::Deserialize)]
        struct Raw {
            title: String,
            #[serde(default)]
            parent: Option<String>,
            #[serde(default = "default_open")]
            open: bool,
            #[serde(default)]
            deadline: Option<Date>,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(Self { title: raw.title, parent: raw.parent, open: raw.open, deadline: raw.deadline })
    }
}

const fn default_open() -> bool {
    true
}

impl ItemDraft {
    fn build(self, id: ItemId) -> Result<Item, MnemonicError> {
        let parent = match self.parent {
            Some(text) => Some(ItemId(Mnemonic::parse(&text)?)),
            None => None,
        };
        Ok(Item {
            id,
            title: self.title,
            parent,
            kind: match (self.open, self.deadline) {
                (true, _) => ProjectKind::Open,
                (false, Some(deadline)) => ProjectKind::Closed { deadline },
                (false, None) => ProjectKind::Closed { deadline: civil::date(2030, 12, 31) },
            },
            commitment: Commitment::None,
            tags: Vec::new(),
            status: Status::Active,
            notes: String::new(),
        })
    }
}

