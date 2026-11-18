use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::io;
use std::path::Path;

use speclib::{MnemonicError, Store};

pub(crate) type ServerResult<Value> = Result<Value, ServerError>;

#[derive(Debug)]
pub(crate) enum ServerError {
    Input(io::Error),
    Json(serde_json::Error),
    Mnemonic(MnemonicError),
    Bind(String),
    Header,
}

impl Display for ServerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(error) => write!(f, "io error {error}"),
            Self::Json(error) => write!(f, "bad json {error}"),
            Self::Mnemonic(error) => write!(f, "bad id {error}"),
            Self::Bind(text) => f.write_str(text),
            Self::Header => f.write_str("bad mime header"),
        }
    }
}

impl Error for ServerError {}

impl From<io::Error> for ServerError {
    fn from(error: io::Error) -> Self {
        Self::Input(error)
    }
}

impl From<serde_json::Error> for ServerError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<MnemonicError> for ServerError {
    fn from(error: MnemonicError) -> Self {
        Self::Mnemonic(error)
    }
}

pub(crate) fn load(store_path: &str) -> ServerResult<Store> {
    match fs::read_to_string(store_path) {
        Ok(text) => Ok(serde_json::from_str(&text)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Store::empty()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn save(store: &Store, store_path: &str) -> ServerResult<()> {
    let staged_path = format!("{store_path}.staged");
    let text = serde_json::to_string_pretty(store)?;
    fs::write(&staged_path, text)?;
    fs::rename(Path::new(&staged_path), Path::new(store_path))?;
    Ok(())
}
