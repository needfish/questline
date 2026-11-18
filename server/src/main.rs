mod api;
mod extract;
mod static_files;
mod store_file;

use std::env;
use std::path::Path;
use std::process;

use crate::static_files::DIST;
use crate::store_file::{ServerResult, ServerError};


fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn run() -> ServerResult<()> {
    let store_path = env::args().nth(1).unwrap_or_else(|| "questline.json".to_owned());
    let port = env::args().nth(2).unwrap_or_else(|| "8080".to_owned());
    if !Path::new(DIST).join("index.html").exists() {
        return Err(ServerError::Bind("missing web build; run npm run build in web".to_owned()));
    }
    let mut store = store_file::load(&store_path)?;
    let server = tiny_http::Server::http(format!("127.0.0.1:{port}")).map_err(|error| ServerError::Bind(error.to_string()))?;
    println!("questline server listening on port {port}");
    for request in server.incoming_requests() {
        if let Err(error) = api::handle(request, &mut store, &store_path) {
            eprintln!("request failed: {error}");
        }
    }
    Ok(())
}
