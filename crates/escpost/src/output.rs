use std::fs;
use std::io;
use std::path::Path;

use escpost_render::RenderResult;
use serde::Serialize;

use crate::application::{self, ApplicationError};

#[derive(Debug, Serialize)]
struct OutputManifest {
    sheets: Vec<String>,
}

pub(crate) fn write_single(
    rendered: &RenderResult,
    output: &Path,
    selected_sheet: Option<usize>,
) -> application::Result<()> {
    fs::write(output, single_png(rendered, selected_sheet)?).map_err(|source| {
        ApplicationError::WriteOutput {
            path: output.to_path_buf(),
            source,
        }
    })?;
    Ok(())
}

pub(crate) fn single_png(
    rendered: &RenderResult,
    selected_sheet: Option<usize>,
) -> application::Result<&[u8]> {
    select_sheet(rendered, selected_sheet).map(|sheet| sheet.png.as_slice())
}

pub(crate) fn write_all(
    rendered: &RenderResult,
    output_directory: &Path,
) -> application::Result<()> {
    fs::create_dir_all(output_directory).map_err(|source| {
        ApplicationError::CreateOutputDirectory {
            path: output_directory.to_path_buf(),
            source,
        }
    })?;
    remove_previous_manifest(output_directory)?;

    let mut sheet_names = Vec::with_capacity(rendered.sheets.len());
    for (index, sheet) in rendered.sheets.iter().enumerate() {
        let name = format!("sheet-{:03}.png", index + 1);
        let path = output_directory.join(&name);
        fs::write(&path, &sheet.png)
            .map_err(|source| ApplicationError::WriteOutput { path, source })?;
        sheet_names.push(name);
    }

    write_manifest(output_directory, sheet_names)
}

fn remove_previous_manifest(output_directory: &Path) -> application::Result<()> {
    let path = output_directory.join("manifest.json");
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ApplicationError::WriteOutput { path, source }),
    }
}

fn select_sheet(
    rendered: &RenderResult,
    selected_sheet: Option<usize>,
) -> application::Result<&escpost_render::RenderedSheet> {
    match selected_sheet {
        Some(number @ 1..) => {
            rendered
                .sheets
                .get(number - 1)
                .ok_or(ApplicationError::SheetOutOfRange {
                    requested: number,
                    available: rendered.sheets.len(),
                })
        }
        Some(number) => Err(ApplicationError::SheetOutOfRange {
            requested: number,
            available: rendered.sheets.len(),
        }),
        None if rendered.sheets.len() == 1 => Ok(&rendered.sheets[0]),
        None => Err(ApplicationError::MultipleSheets(rendered.sheets.len())),
    }
}

fn write_manifest(output_directory: &Path, sheets: Vec<String>) -> application::Result<()> {
    let manifest = serde_json::to_vec_pretty(&OutputManifest { sheets })?;
    let pending_path = output_directory.join(".manifest.json.tmp");
    let manifest_path = output_directory.join("manifest.json");
    fs::write(&pending_path, [manifest.as_slice(), b"\n"].concat()).map_err(|source| {
        ApplicationError::WriteOutput {
            path: pending_path.clone(),
            source,
        }
    })?;

    // The old manifest was removed before writing sheets. This rename works
    // consistently on Windows and Unix and publishes the completed list last.
    fs::rename(&pending_path, &manifest_path).map_err(|source| ApplicationError::WriteOutput {
        path: manifest_path,
        source,
    })
}
