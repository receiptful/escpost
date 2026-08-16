use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::application::{self, ApplicationError};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum InputFormat {
    #[default]
    Auto,
    Binary,
    Hex,
}

#[derive(Debug)]
pub(crate) struct LoadedSource {
    pub(crate) bytes: Vec<u8>,
    pub(crate) profile: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseManifest {
    schema_version: u32,
    name: String,
    profile: String,
}

pub(crate) fn load(path: &Path, format: InputFormat) -> application::Result<LoadedSource> {
    if path == Path::new("-") {
        return load_stdin(format);
    }
    if path.is_dir() {
        return load_case(path);
    }

    Ok(LoadedSource {
        bytes: load_file(path, format)?,
        profile: None,
    })
}

fn load_stdin(format: InputFormat) -> application::Result<LoadedSource> {
    let mut bytes = Vec::new();
    io::stdin()
        .read_to_end(&mut bytes)
        .map_err(ApplicationError::ReadStdin)?;
    Ok(LoadedSource {
        bytes: match format {
            InputFormat::Hex => decode_hex(&bytes)?,
            InputFormat::Auto | InputFormat::Binary => bytes,
        },
        profile: None,
    })
}

fn load_case(directory: &Path) -> application::Result<LoadedSource> {
    let manifest_path = directory.join("case.toml");
    if !manifest_path.is_file() {
        return Err(ApplicationError::UnrecognizedDirectory(
            directory.to_path_buf(),
        ));
    }
    let manifest_bytes = read(&manifest_path)?;
    let manifest_text = std::str::from_utf8(&manifest_bytes).map_err(|error| {
        ApplicationError::InvalidCaseManifest {
            path: manifest_path.clone(),
            message: error.to_string(),
        }
    })?;
    let manifest: CaseManifest =
        toml::from_str(manifest_text).map_err(|error| ApplicationError::InvalidCaseManifest {
            path: manifest_path,
            message: error.to_string(),
        })?;
    validate_case_manifest(&manifest)?;

    Ok(LoadedSource {
        bytes: load_file(&directory.join("input.hex"), InputFormat::Hex)?,
        profile: Some(manifest.profile),
    })
}

fn validate_case_manifest(manifest: &CaseManifest) -> application::Result<()> {
    if manifest.schema_version != 1 {
        return Err(ApplicationError::UnsupportedCaseSchema(
            manifest.schema_version,
        ));
    }
    if manifest.name.is_empty() {
        return Err(ApplicationError::EmptyCaseField("name"));
    }
    if manifest.profile.is_empty() {
        return Err(ApplicationError::EmptyCaseField("profile"));
    }
    Ok(())
}

fn load_file(path: &Path, format: InputFormat) -> application::Result<Vec<u8>> {
    let bytes = read(path)?;
    let format = match format {
        InputFormat::Auto if has_hex_extension(path) => InputFormat::Hex,
        InputFormat::Auto => InputFormat::Binary,
        explicit => explicit,
    };

    match format {
        InputFormat::Auto | InputFormat::Binary => Ok(bytes),
        InputFormat::Hex => decode_hex(&bytes),
    }
}

fn read(path: &Path) -> application::Result<Vec<u8>> {
    fs::read(path).map_err(|source| {
        if source.kind() == io::ErrorKind::NotFound {
            ApplicationError::InputFileNotFound {
                path: PathBuf::from(path),
                source,
            }
        } else {
            ApplicationError::ReadInput {
                path: PathBuf::from(path),
                source,
            }
        }
    })
}

fn has_hex_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("hex"))
}

fn decode_hex(input: &[u8]) -> application::Result<Vec<u8>> {
    let input = std::str::from_utf8(input)?;
    input
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| decode_hex_byte(token, index + 1))
        .collect()
}

fn decode_hex_byte(token: &str, position: usize) -> application::Result<u8> {
    let valid = token.len() == 2 && token.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid {
        return Err(ApplicationError::InvalidHexByte {
            token: token.to_owned(),
            position,
        });
    }
    u8::from_str_radix(token, 16).map_err(|_| ApplicationError::InvalidHexByte {
        token: token.to_owned(),
        position,
    })
}
