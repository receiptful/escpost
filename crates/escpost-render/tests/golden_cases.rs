use escpost_profiles::resolver;
use escpost_render::render;
use serde::Deserialize;
use std::fs;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseManifest {
    schema_version: u32,
    name: String,
    profile: String,
}

#[derive(Debug)]
struct DecodedPng {
    width: u32,
    height: u32,
    printed: Vec<bool>,
}

#[derive(Debug)]
struct PixelDifference {
    changed_dots: usize,
    first_change: (u32, u32),
    bounds: (u32, u32, u32, u32),
}

#[test]
fn conformance_cases_match_their_golden_pngs() {
    let repository = repository_root();
    // Match repository_root's canonical form (including Windows' \\?\ prefix)
    // so the comparison report can strip the repository prefix reliably.
    let cases_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/cases")
        .canonicalize()
        .expect("the conformance cases directory should exist");
    let output_root = repository.join(".test-output");
    let mut case_directories = find_case_directories(&cases_root);
    case_directories.sort();

    assert!(
        !case_directories.is_empty(),
        "no conformance cases found under {}",
        display_path(&cases_root, &repository)
    );

    let mut failures = Vec::new();
    for case_directory in case_directories {
        if let Err(error) = compare_case(&case_directory, &cases_root, &output_root, &repository) {
            failures.push(error);
        }
    }

    assert!(
        failures.is_empty(),
        "golden PNG comparison failed:\n\n{}",
        failures.join("\n\n")
    );
}

#[test]
fn visual_diff_marks_missing_dots_blue_and_unexpected_dots_red() {
    let expected = DecodedPng {
        width: 2,
        height: 1,
        printed: vec![true, false],
    };
    let actual = DecodedPng {
        width: 2,
        height: 1,
        printed: vec![false, true],
    };

    let difference = compare_pixels(&expected, &actual).expect("the two pixels differ");
    assert_eq!(difference.changed_dots, 2);
    assert_eq!(difference.first_change, (0, 0));
    assert_eq!(difference.bounds, (0, 0, 1, 0));

    let path = std::env::temp_dir().join(format!("escpost-diff-{}.png", std::process::id()));
    write_diff_png(&expected, &actual, &path).expect("the visual diff should encode");
    let pixels = decode_rgb_png(&path);
    fs::remove_file(&path).expect("the temporary visual diff should be removable");

    assert_eq!(pixels, [0, 102, 255, 255, 0, 0]);
}

fn compare_case(
    case_directory: &Path,
    cases_root: &Path,
    output_root: &Path,
    repository: &Path,
) -> Result<(), String> {
    let manifest = load_manifest(case_directory)?;
    let input = load_input_file(&case_directory.join("input.hex"), &manifest.name)?;
    let relative_case = case_directory
        .strip_prefix(cases_root)
        .expect("discovered cases stay under the cases root");
    let output_directory = output_root.join(relative_case);

    compare_receipt(
        &manifest.name,
        &manifest.profile,
        &input,
        case_directory,
        &output_directory,
        repository,
    )
}

fn compare_receipt(
    name: &str,
    profile_id: &str,
    input: &[u8],
    expected_directory: &Path,
    output_directory: &Path,
    repository: &Path,
) -> Result<(), String> {
    let expected_sheets = find_expected_sheets(expected_directory)?;
    let profile = load_profile(profile_id)?;
    let rendered =
        render(input, &profile).map_err(|error| format!("{name}: render failed: {error}"))?;
    fs::create_dir_all(output_directory).map_err(|error| {
        format!(
            "{name}: could not create {}: {error}",
            display_path(output_directory, repository)
        )
    })?;
    clear_generated_artifacts(output_directory)?;

    let mut comparison_rows = Vec::new();
    let mut failures = Vec::new();
    for (index, sheet) in rendered.sheets.iter().enumerate() {
        let sheet_number = index + 1;
        let actual_path = output_directory.join(format!("actual-{sheet_number:03}.png"));
        fs::write(&actual_path, &sheet.png).map_err(|error| {
            format!(
                "{name}: could not write {}: {error}",
                display_path(&actual_path, repository)
            )
        })?;

        let expected_path = expected_sheets.get(index);
        comparison_rows.push(comparison_row(
            sheet_number,
            expected_path.map(PathBuf::as_path),
            &actual_path,
            None,
            output_directory,
            repository,
        ));

        let Some(expected_path) = expected_path else {
            failures.push(format!(
                "{} sheet {sheet_number:03}: unexpected generated sheet\nactual: {}",
                name,
                display_path(&actual_path, repository)
            ));
            continue;
        };
        let expected_png = match decode_png_file(expected_path) {
            Ok(png) => png,
            Err(error) => {
                failures.push(format!(
                    "{} sheet {sheet_number:03}: {error}\nexpected: {}\nactual:   {}",
                    name,
                    display_path(expected_path, repository),
                    display_path(&actual_path, repository)
                ));
                continue;
            }
        };
        let actual_png = decode_png_bytes(&sheet.png).map_err(|error| {
            format!(
                "{} sheet {sheet_number:03}: generated PNG is invalid: {error}",
                name
            )
        })?;

        if let Some(difference) = compare_pixels(&expected_png, &actual_png) {
            let diff_path = output_directory.join(format!("diff-{sheet_number:03}.png"));
            write_diff_png(&expected_png, &actual_png, &diff_path).map_err(|error| {
                format!(
                    "{name}: could not write {}: {error}",
                    display_path(&diff_path, repository)
                )
            })?;
            comparison_rows[index] = comparison_row(
                sheet_number,
                Some(expected_path),
                &actual_path,
                Some(&diff_path),
                output_directory,
                repository,
            );
            failures.push(format_difference(
                name,
                sheet_number,
                &difference,
                expected_path,
                &actual_path,
                &diff_path,
                repository,
            ));
        }
    }

    if rendered.sheets.len() < expected_sheets.len() {
        for (index, expected_path) in expected_sheets
            .iter()
            .enumerate()
            .skip(rendered.sheets.len())
        {
            let sheet_number = index + 1;
            failures.push(format!(
                "{} sheet {sheet_number:03}: expected sheet was not generated\nexpected: {}",
                name,
                display_path(expected_path, repository)
            ));
        }
    }

    let comparison_path = output_directory.join("comparison.html");
    write_comparison_html(
        name,
        failures.is_empty(),
        &comparison_rows,
        &comparison_path,
    )
    .map_err(|error| {
        format!(
            "{name}: could not write {}: {error}",
            display_path(&comparison_path, repository)
        )
    })?;

    if failures.is_empty() {
        return Ok(());
    }

    failures.push(format!(
        "compare:  {}",
        display_path(&comparison_path, repository)
    ));
    Err(failures.join("\n"))
}

fn load_manifest(case_directory: &Path) -> Result<CaseManifest, String> {
    let path = case_directory.join("case.toml");
    let source =
        fs::read_to_string(&path).map_err(|error| format!("could not read {path:?}: {error}"))?;
    let manifest: CaseManifest =
        toml::from_str(&source).map_err(|error| format!("invalid {path:?}: {error}"))?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "unsupported case schema version {}",
            manifest.schema_version
        ));
    }
    Ok(manifest)
}

fn load_input_file(path: &Path, receipt_name: &str) -> Result<Vec<u8>, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("{receipt_name}: could not read {path:?}: {error}"))?;
    let mut input = Vec::new();
    for (index, token) in source.split_whitespace().enumerate() {
        if token.len() != 2 {
            return Err(format!(
                "{}: invalid hex byte {token:?} at token {}",
                receipt_name,
                index + 1
            ));
        }
        let byte = u8::from_str_radix(token, 16).map_err(|_| {
            format!(
                "{}: invalid hex byte {token:?} at token {}",
                receipt_name,
                index + 1
            )
        })?;
        input.push(byte);
    }

    Ok(input)
}

fn find_expected_sheets(case_directory: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(case_directory)
        .map_err(|error| format!("could not inspect {case_directory:?}: {error}"))?;
    let mut sheets = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("expected-") && name.ends_with(".png"))
        })
        .collect::<Vec<_>>();
    sheets.sort();
    if sheets.is_empty() {
        return Err(format!(
            "case {} has no expected-NNN.png files",
            display_path(case_directory, &repository_root())
        ));
    }
    Ok(sheets)
}

fn load_profile(profile: &str) -> Result<escpost_profiles::PrinterProfile, String> {
    resolver::resolve(profile)
        .cloned()
        .map_err(|error| format!("could not resolve profile {profile:?}: {error}"))
}

fn decode_png_file(path: &Path) -> Result<DecodedPng, String> {
    let bytes = fs::read(path).map_err(|error| format!("could not read golden PNG: {error}"))?;
    decode_png_bytes(&bytes).map_err(|error| format!("could not decode golden PNG: {error}"))
}

fn decode_png_bytes(bytes: &[u8]) -> Result<DecodedPng, String> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|error| format!("invalid PNG header: {error}"))?;
    let mut pixels = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or("decoded PNG dimensions exceed memory limits")?
    ];
    let info = reader
        .next_frame(&mut pixels)
        .map_err(|error| format!("invalid PNG pixels: {error}"))?;
    if info.color_type != png::ColorType::Grayscale || info.bit_depth != png::BitDepth::One {
        return Err(format!(
            "expected one-bit grayscale PNG, got {:?} {:?}",
            info.color_type, info.bit_depth
        ));
    }

    let row_bytes = info.width.div_ceil(8) as usize;
    let mut printed = Vec::with_capacity((info.width * info.height) as usize);
    for y in 0..info.height as usize {
        for x in 0..info.width as usize {
            let byte = pixels[y * row_bytes + x / 8];
            printed.push(byte & (0x80 >> (x % 8)) == 0);
        }
    }
    Ok(DecodedPng {
        width: info.width,
        height: info.height,
        printed,
    })
}

fn compare_pixels(expected: &DecodedPng, actual: &DecodedPng) -> Option<PixelDifference> {
    let width = expected.width.max(actual.width);
    let height = expected.height.max(actual.height);
    let mut changed_dots = 0;
    let mut first_change = None;
    let mut bounds = (u32::MAX, u32::MAX, 0, 0);

    for y in 0..height {
        for x in 0..width {
            let inside_expected = x < expected.width && y < expected.height;
            let inside_actual = x < actual.width && y < actual.height;
            if inside_expected == inside_actual && pixel(expected, x, y) == pixel(actual, x, y) {
                continue;
            }

            changed_dots += 1;
            first_change.get_or_insert((x, y));
            bounds.0 = bounds.0.min(x);
            bounds.1 = bounds.1.min(y);
            bounds.2 = bounds.2.max(x);
            bounds.3 = bounds.3.max(y);
        }
    }

    first_change.map(|first_change| PixelDifference {
        changed_dots,
        first_change,
        bounds,
    })
}

fn pixel(image: &DecodedPng, x: u32, y: u32) -> bool {
    if x >= image.width || y >= image.height {
        return false;
    }
    image.printed[(y * image.width + x) as usize]
}

fn write_diff_png(expected: &DecodedPng, actual: &DecodedPng, path: &Path) -> Result<(), String> {
    let width = expected.width.max(actual.width);
    let height = expected.height.max(actual.height);
    let byte_count = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(3);
    let capacity =
        usize::try_from(byte_count).map_err(|_| "visual diff is too large".to_owned())?;
    let mut pixels = Vec::with_capacity(capacity);
    for y in 0..height {
        for x in 0..width {
            let color = match (pixel(expected, x, y), pixel(actual, x, y)) {
                (true, true) => [0, 0, 0],
                (false, false) => [255, 255, 255],
                (false, true) => [255, 0, 0],
                (true, false) => [0, 102, 255],
            };
            pixels.extend_from_slice(&color);
        }
    }

    let file = fs::File::create(path).map_err(|error| error.to_string())?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    writer
        .write_image_data(&pixels)
        .map_err(|error| error.to_string())
}

fn format_difference(
    case_name: &str,
    sheet_number: usize,
    difference: &PixelDifference,
    expected: &Path,
    actual: &Path,
    diff: &Path,
    repository: &Path,
) -> String {
    format!(
        "{case_name} sheet {sheet_number:03}: PNG pixels differ\n\
         expected: {}\n\
         actual:   {}\n\
         diff:     {}\n\
         changed dots: {}\n\
         first change: ({}, {})\n\
         difference bounds: x={}..{}, y={}..{}",
        display_path(expected, repository),
        display_path(actual, repository),
        display_path(diff, repository),
        difference.changed_dots,
        difference.first_change.0,
        difference.first_change.1,
        difference.bounds.0,
        difference.bounds.2,
        difference.bounds.1,
        difference.bounds.3,
    )
}

fn comparison_row(
    sheet_number: usize,
    expected: Option<&Path>,
    actual: &Path,
    diff: Option<&Path>,
    output_directory: &Path,
    repository: &Path,
) -> String {
    let expected = expected
        .map(|path| relative_url(path, output_directory, repository))
        .unwrap_or_default();
    let actual = relative_url(actual, output_directory, repository);
    let diff_figure = diff.map_or_else(
        || "<figure><figcaption>Diff</figcaption><p>No differences</p></figure>".to_owned(),
        |path| {
            let diff = relative_url(path, output_directory, repository);
            format!("<figure><figcaption>Diff</figcaption><img src=\"{diff}\"></figure>")
        },
    );
    format!(
        "<section><h2>Sheet {sheet_number:03}</h2>\
         <div class=\"images\">\
         <figure><figcaption>Expected</figcaption><img src=\"{expected}\"></figure>\
         <figure><figcaption>Actual</figcaption><img src=\"{actual}\"></figure>\
         {diff_figure}\
         </div></section>"
    )
}

fn write_comparison_html(
    case_name: &str,
    matches: bool,
    rows: &[String],
    path: &Path,
) -> Result<(), std::io::Error> {
    let status = if matches { "matches" } else { "differs" };
    let mut html = format!(
        "<!doctype html><meta charset=\"utf-8\">\
         <title>{case_name} — {status}</title>\
         <style>\
         body{{font-family:sans-serif;margin:2rem;background:#eee}}\
         .images{{display:flex;gap:1rem;align-items:flex-start;overflow:auto}}\
         figure{{margin:0}}\
         img{{image-rendering:pixelated;background:white;border:1px solid #888;min-width:384px}}\
         </style><h1>{case_name}</h1><p>Status: {status}</p>"
    );
    for row in rows {
        html.push_str(row);
    }
    fs::write(path, html)
}

fn relative_url(path: &Path, output_directory: &Path, repository: &Path) -> String {
    if let Ok(local_path) = path.strip_prefix(output_directory) {
        return local_path.to_string_lossy().replace('\\', "/");
    }

    let repository_path = path
        .strip_prefix(repository)
        .expect("comparison paths stay inside the repository");
    let output_depth = output_directory
        .strip_prefix(repository)
        .expect("output stays inside the repository")
        .components()
        .count();
    format!(
        "{}{}",
        "../".repeat(output_depth),
        repository_path.to_string_lossy().replace('\\', "/")
    )
}

fn find_case_directories(root: &Path) -> Vec<PathBuf> {
    let mut cases = Vec::new();
    visit_case_directories(root, &mut cases);
    cases
}

fn clear_generated_artifacts(directory: &Path) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("could not inspect generated artifacts: {error}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let generated_png =
            (name.starts_with("actual-") || name.starts_with("diff-")) && name.ends_with(".png");
        if generated_png || name == "comparison.html" {
            fs::remove_file(&path).map_err(|error| {
                format!("could not replace generated artifact {path:?}: {error}")
            })?;
        }
    }
    Ok(())
}

fn decode_rgb_png(path: &Path) -> Vec<u8> {
    let file = fs::File::open(path).expect("the visual diff should exist");
    let decoder = png::Decoder::new(BufReader::new(file));
    let mut reader = decoder
        .read_info()
        .expect("the visual diff should be valid");
    let mut pixels = vec![
        0;
        reader
            .output_buffer_size()
            .expect("the visual diff should fit in memory")
    ];
    let info = reader
        .next_frame(&mut pixels)
        .expect("the visual diff should contain pixels");

    assert_eq!(info.color_type, png::ColorType::Rgb);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    pixels.truncate(info.buffer_size());
    pixels
}

fn visit_case_directories(directory: &Path, cases: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_case_directories(&path, cases);
        } else if path.file_name().is_some_and(|name| name == "case.toml") {
            cases.push(directory.to_owned());
        }
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repository root should exist")
}

fn display_path(path: &Path, repository: &Path) -> String {
    path.strip_prefix(repository)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
