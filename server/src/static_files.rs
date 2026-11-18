use std::fs;
use std::path::{Component, Path, PathBuf};

use tiny_http::{Request, Response};

use crate::store_file::{ServerResult, ServerError};

pub(crate) const DIST: &str = "web";

pub(crate) fn serve(request: Request, url: &str) -> ServerResult<()> {
    let relative = match url {
        "/" => "index.html",
        other => other.trim_start_matches('/'),
    };
    let loaded = resolve(relative).and_then(|file| fs::read(&file).ok().map(|bytes| (bytes, mime_of(&file))));
    let Some((bytes, mime)) = loaded else {
        let Ok(header) = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/plain"[..]) else {
            return Err(ServerError::Header);
        };
        let text = "not found".to_owned();
        request.respond(Response::from_string(text).with_status_code(404).with_header(header))?;
        return Ok(());
    };
    let Ok(header) = tiny_http::Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()) else {
        return Err(ServerError::Header);
    };
    request.respond(Response::from_data(bytes).with_header(header))?;
    Ok(())
}

fn resolve(relative: &str) -> Option<PathBuf> {
    let path = Path::new(relative);
    if path.components().any(|part| matches!(part, Component::ParentDir | Component::RootDir)) {
        return None;
    }
    Some(Path::new(DIST).join(path))
}

fn mime_of(file: &Path) -> String {
    let extension = file.extension().and_then(|name| name.to_str()).unwrap_or("");
    match extension {
        "html" => "text/html",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "woff2" => "font/woff2",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
    .to_owned()
}
