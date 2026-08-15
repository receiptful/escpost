//! Terminal adapter for the profile catalog operations.

use std::io::IsTerminal;

use clap::{Args, Subcommand, ValueEnum};
use escpost_profiles::{BarcodeSystem, ProfileSource};
use inquire::Select;
use serde::Serialize;

use crate::error::CliError;

use super::{
    BarcodeFacts, FeaturesFacts, GetRequest, ListRequest, ProfileFacts, ProfileSourceFilter, get,
    list,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum SourceFilter {
    Calibrated,
    Synthesized,
    Virtual,
}

#[derive(Debug, Args)]
pub(crate) struct ProfilesArgs {
    #[command(subcommand)]
    pub(crate) command: ProfilesCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ProfilesCommand {
    /// List available printer profiles.
    List(ListProfilesArgs),
    /// Get the full details of a single printer profile.
    Get(GetProfileArgs),
    /// Interactively pick a profile and print its id.
    Find(FindProfileArgs),
}

#[derive(Debug, Args)]
pub(crate) struct FindProfileArgs {}

#[derive(Debug, Args)]
pub(crate) struct ListProfilesArgs {
    /// Show only profiles from this vendor (case-insensitive substring).
    #[arg(long)]
    pub(crate) vendor: Option<String>,

    /// Show only profiles with this calibration provenance.
    #[arg(long, value_enum)]
    pub(crate) source: Option<SourceFilter>,

    /// Show only profiles whose id, vendor, or model contains this text
    /// (case-insensitive).
    #[arg(long)]
    pub(crate) search: Option<String>,

    /// Print the full profile catalog as JSON instead of a table.
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct GetProfileArgs {
    /// Profile id (as passed to --profile).
    pub(crate) id: String,

    /// Print the profile as JSON instead of the detail view.
    #[arg(long)]
    pub(crate) json: bool,
}

pub(crate) fn run(arguments: ProfilesArgs, non_interactive: bool) -> Result<(), CliError> {
    match arguments.command {
        ProfilesCommand::List(arguments) => run_list(arguments),
        ProfilesCommand::Get(arguments) => run_get(arguments),
        ProfilesCommand::Find(arguments) => run_find(arguments, non_interactive),
    }
}

fn run_list(arguments: ListProfilesArgs) -> Result<(), CliError> {
    let response = list(ListRequest {
        vendor: arguments.vendor,
        source: arguments.source.map(profile_source_filter),
        search: arguments.search,
    })?;

    if response.profiles.is_empty() && !arguments.json {
        eprintln!("no profiles match");
        return Ok(());
    }

    if arguments.json {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &response
                    .profiles
                    .iter()
                    .map(ProfileJson::from)
                    .collect::<Vec<_>>(),
            )
            .map_err(CliError::SerializeJsonOutput)?
        );
    } else {
        println!("{}", render_table(&response.profiles));
    }
    Ok(())
}

fn run_get(arguments: GetProfileArgs) -> Result<(), CliError> {
    let response = get(GetRequest { id: arguments.id })?;

    if arguments.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ProfileJson::from(&response.profile))
                .map_err(CliError::SerializeJsonOutput)?
        );
    } else {
        println!("{}", render_detail(&response.profile));
    }
    Ok(())
}

fn run_find(_arguments: FindProfileArgs, non_interactive: bool) -> Result<(), CliError> {
    let can_prompt =
        !non_interactive && std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    if !can_prompt {
        return Err(CliError::InteractiveFindUnavailable);
    }

    let response = list(ListRequest {
        vendor: None,
        source: None,
        search: None,
    })?;
    let options: Vec<(String, String)> = response
        .profiles
        .into_iter()
        .map(|profile| {
            (
                format!("{} — {} · {}", profile.id, profile.vendor, profile.model),
                profile.id,
            )
        })
        .collect();
    let labels: Vec<String> = options.iter().map(|(label, _)| label.clone()).collect();

    let chosen = Select::new("Find a printer profile", labels)
        .with_page_size(10)
        .prompt()
        .map_err(|error| CliError::ProfilePrompt(error.to_string()))?;

    let id = options
        .into_iter()
        .find(|(label, _)| *label == chosen)
        .map(|(_, id)| id)
        .expect("inquire returns one of the supplied labels");
    println!("{id}");
    Ok(())
}

fn profile_source_filter(filter: SourceFilter) -> ProfileSourceFilter {
    match filter {
        SourceFilter::Calibrated => ProfileSourceFilter::Calibrated,
        SourceFilter::Synthesized => ProfileSourceFilter::Synthesized,
        SourceFilter::Virtual => ProfileSourceFilter::Virtual,
    }
}

const TABLE_HEADERS: [&str; 11] = [
    "PROFILE", "VENDOR", "MODEL", "CAL", "PAPER", "PRINT", "DOTS", "DPI", "CUT", "BC", "QR",
];
const TABLE_LEGEND: &str =
    "CAL: ✓ calibrated · ~ synthesized · ○ virtual   PAPER/PRINT mm, DOTS printable";

fn render_table(views: &[ProfileFacts]) -> String {
    let mut rows: Vec<[String; 11]> = vec![TABLE_HEADERS.map(str::to_owned)];
    rows.extend(views.iter().map(table_row));

    let mut widths = [0usize; 11];
    for row in &rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.chars().count());
        }
    }

    let mut lines: Vec<String> = rows
        .iter()
        .map(|row| format_table_row(row, &widths))
        .collect();
    lines.push(String::new());
    lines.push(TABLE_LEGEND.to_owned());
    lines.join("\n")
}

fn format_table_row(cells: &[String; 11], widths: &[usize; 11]) -> String {
    cells
        .iter()
        .zip(widths.iter())
        .map(|(cell, width)| format!("{cell:<width$}"))
        .collect::<Vec<_>>()
        .join("  ")
        .trim_end()
        .to_owned()
}

fn table_row(view: &ProfileFacts) -> [String; 11] {
    [
        view.id.clone(),
        view.vendor.clone(),
        view.model.clone(),
        calibration_marker(&view.source).to_owned(),
        format!("{:.1}", view.paper_width_mm),
        format!("{:.1}", view.printable_width_mm),
        view.printable_width_dots.to_string(),
        view.dpi_x.to_string(),
        cut_marker(&view.features).to_owned(),
        barcode_marker(&view.features.barcodes).to_owned(),
        check_marker(view.features.qr_code).to_owned(),
    ]
}

fn source_label(source: &ProfileSource) -> &'static str {
    match source {
        ProfileSource::Upstream => "calibrated",
        ProfileSource::UpstreamDefault => "synthesized",
        ProfileSource::Reference => "virtual",
    }
}

fn calibration_marker(source: &ProfileSource) -> &'static str {
    match source {
        ProfileSource::Upstream => "✓",
        ProfileSource::UpstreamDefault => "~",
        ProfileSource::Reference => "○",
    }
}

fn cut_marker(features: &FeaturesFacts) -> &'static str {
    check_marker(features.paper_full_cut || features.paper_part_cut)
}

fn check_marker(supported: bool) -> &'static str {
    if supported { "✓" } else { "–" }
}

fn barcode_marker(barcodes: &BarcodeFacts) -> &'static str {
    match (
        !barcodes.function_a.is_empty(),
        !barcodes.function_b.is_empty(),
    ) {
        (true, true) => "A·B",
        (true, false) => "A",
        (false, true) => "B",
        (false, false) => "–",
    }
}

fn render_detail(view: &ProfileFacts) -> String {
    let marker = calibration_marker(&view.source);
    let lines = [
        format!("{} — {} {}", view.id, view.vendor, view.model),
        format!("source:           {} {marker}", source_label(&view.source)),
        format!("sha256:           {}", view.canonical_profile_sha256),
        String::new(),
        format!("paper width:      {:.1} mm", view.paper_width_mm),
        format!(
            "printable width:  {:.1} mm ({} dots)",
            view.printable_width_mm, view.printable_width_dots
        ),
        format!("resolution:       {} x {} dpi", view.dpi_x, view.dpi_y),
        String::new(),
        format!(
            "font a:           {}x{} dots, baseline {}",
            view.fonts.a.cell_width_dots, view.fonts.a.cell_height_dots, view.fonts.a.baseline_dots
        ),
        format!(
            "font b:           {}x{} dots, baseline {}",
            view.fonts.b.cell_width_dots, view.fonts.b.cell_height_dots, view.fonts.b.baseline_dots
        ),
        String::new(),
        format!("code pages:       {}", view.code_page_count),
        String::new(),
        format!("graphics:         {}", yes_no(view.features.graphics)),
        format!(
            "cut:              full {} · partial {}",
            yes_no(view.features.paper_full_cut),
            yes_no(view.features.paper_part_cut)
        ),
        format!("qr code:          {}", yes_no(view.features.qr_code)),
        format!("drawer pulse:     {}", yes_no(view.features.pulse_standard)),
        format!(
            "barcodes (a):     {}",
            barcode_list(&view.features.barcodes.function_a)
        ),
        format!(
            "barcodes (b):     {}",
            barcode_list(&view.features.barcodes.function_b)
        ),
    ];
    lines.join("\n")
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn barcode_list(systems: &std::collections::BTreeSet<BarcodeSystem>) -> String {
    if systems.is_empty() {
        "none".to_owned()
    } else {
        systems
            .iter()
            .map(barcode_system_label)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn barcode_system_label(system: &BarcodeSystem) -> &'static str {
    match system {
        BarcodeSystem::UpcA => "upc_a",
        BarcodeSystem::UpcE => "upc_e",
        BarcodeSystem::Ean13 => "ean_13",
        BarcodeSystem::Ean8 => "ean_8",
        BarcodeSystem::Code39 => "code_39",
        BarcodeSystem::Itf => "itf",
        BarcodeSystem::Codabar => "codabar",
        BarcodeSystem::Code93 => "code_93",
        BarcodeSystem::Code128 => "code_128",
        BarcodeSystem::Gs1_128 => "gs1_128",
        BarcodeSystem::Gs1DataBarOmnidirectional => "gs1_databar_omnidirectional",
        BarcodeSystem::Gs1DataBarTruncated => "gs1_databar_truncated",
        BarcodeSystem::Gs1DataBarLimited => "gs1_databar_limited",
        BarcodeSystem::Gs1DataBarExpanded => "gs1_databar_expanded",
        BarcodeSystem::Code128Auto => "code_128_auto",
    }
}

#[derive(Serialize)]
struct ProfileJson {
    id: String,
    vendor: String,
    model: String,
    source: &'static str,
    paper_width_mm: f64,
    printable_width_mm: f64,
    printable_width_dots: u32,
    dpi_x: u32,
    dpi_y: u32,
    fonts: FontsJson,
    features: FeaturesJson,
    code_page_count: usize,
    canonical_profile_sha256: String,
}

#[derive(Serialize)]
struct FontsJson {
    a: FontJson,
    b: FontJson,
}

#[derive(Serialize)]
struct FontJson {
    cell_width_dots: u32,
    cell_height_dots: u32,
    baseline_dots: u32,
}

#[derive(Serialize)]
struct FeaturesJson {
    barcodes: BarcodesJson,
    graphics: bool,
    paper_full_cut: bool,
    paper_part_cut: bool,
    qr_code: bool,
    pulse_standard: bool,
}

#[derive(Serialize)]
struct BarcodesJson {
    function_a: Vec<&'static str>,
    function_b: Vec<&'static str>,
}

impl From<&BarcodeFacts> for BarcodesJson {
    fn from(facts: &BarcodeFacts) -> Self {
        Self {
            function_a: facts.function_a.iter().map(barcode_system_label).collect(),
            function_b: facts.function_b.iter().map(barcode_system_label).collect(),
        }
    }
}

impl From<&ProfileFacts> for ProfileJson {
    fn from(facts: &ProfileFacts) -> Self {
        Self {
            id: facts.id.clone(),
            vendor: facts.vendor.clone(),
            model: facts.model.clone(),
            source: source_label(&facts.source),
            paper_width_mm: facts.paper_width_mm,
            printable_width_mm: facts.printable_width_mm,
            printable_width_dots: facts.printable_width_dots,
            dpi_x: facts.dpi_x,
            dpi_y: facts.dpi_y,
            fonts: FontsJson {
                a: FontJson {
                    cell_width_dots: facts.fonts.a.cell_width_dots,
                    cell_height_dots: facts.fonts.a.cell_height_dots,
                    baseline_dots: facts.fonts.a.baseline_dots,
                },
                b: FontJson {
                    cell_width_dots: facts.fonts.b.cell_width_dots,
                    cell_height_dots: facts.fonts.b.cell_height_dots,
                    baseline_dots: facts.fonts.b.baseline_dots,
                },
            },
            features: FeaturesJson {
                barcodes: BarcodesJson::from(&facts.features.barcodes),
                graphics: facts.features.graphics,
                paper_full_cut: facts.features.paper_full_cut,
                paper_part_cut: facts.features.paper_part_cut,
                qr_code: facts.features.qr_code,
                pulse_standard: facts.features.pulse_standard,
            },
            code_page_count: facts.code_page_count,
            canonical_profile_sha256: facts.canonical_profile_sha256.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use escpost_profiles::resolver;

    use super::*;

    #[test]
    fn table_includes_the_calibration_marker_and_legend() {
        let profile = resolver::resolve("TM-T88III").expect("the fixture profile should resolve");
        let view = ProfileFacts::from_profile(profile);

        let table = render_table(std::slice::from_ref(&view));

        assert!(table.contains("TM-T88III"));
        assert!(table.contains('~'), "synthesized profiles use ~: {table}");
        assert!(table.contains("CAL: "));
    }

    #[test]
    fn detail_shows_decimal_millimeters_and_barcode_lists() {
        let profile = resolver::resolve("TM-T88III").expect("the fixture profile should resolve");
        let view = ProfileFacts::from_profile(profile);

        let detail = render_detail(&view);

        assert!(detail.contains("80.0 mm"));
        assert!(detail.contains("72.2 mm"));
        assert!(detail.contains("code_128") || detail.contains("upc_a"));
    }

    #[test]
    fn barcode_json_projects_domain_values_to_adapter_labels() {
        let facts = BarcodeFacts {
            function_a: BTreeSet::from([BarcodeSystem::UpcA, BarcodeSystem::Code39]),
            function_b: BTreeSet::from([BarcodeSystem::Code93, BarcodeSystem::Code128]),
        };

        let json = serde_json::to_value(BarcodesJson::from(&facts))
            .expect("adapter barcode labels should serialize");

        assert_eq!(
            json,
            serde_json::json!({
                "function_a": ["upc_a", "code_39"],
                "function_b": ["code_93", "code_128"]
            })
        );
    }
}
