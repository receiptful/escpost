//! Typed operations for the embedded printer-profile catalog.

use std::collections::BTreeSet;

use escpost_profiles::resolver::{self, ResolveError};
use escpost_profiles::{BarcodeSystem, Font, PrinterProfile, ProfileSource};
use serde::Serialize;

use crate::application;
use crate::error::CliError;

pub mod cli;
pub use cli::{render_detail, render_table};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProfileSourceFilter {
    Calibrated,
    Synthesized,
    Virtual,
}

pub(crate) struct ListRequest {
    pub(crate) vendor: Option<String>,
    pub(crate) source: Option<ProfileSourceFilter>,
    pub(crate) search: Option<String>,
}

pub(crate) struct ListResponse {
    pub(crate) profiles: Vec<ProfileView>,
}

pub(crate) struct ShowRequest {
    pub(crate) id: String,
}

pub(crate) struct ShowResponse {
    pub(crate) profile: ProfileView,
}

pub(crate) fn list(request: ListRequest) -> application::Result<ListResponse> {
    let profiles = all_views()?
        .into_iter()
        .filter(|profile| matches_filters(profile, &request))
        .collect();
    Ok(ListResponse { profiles })
}

pub(crate) fn show(request: ShowRequest) -> application::Result<ShowResponse> {
    let profile = resolver::resolve(&request.id)
        .map(ProfileView::from_profile)
        .map_err(map_resolve_error)?;
    Ok(ShowResponse { profile })
}

fn all_views() -> application::Result<Vec<ProfileView>> {
    let ids = resolver::available_ids().map_err(map_resolve_error)?;
    let mut views = ids
        .iter()
        .map(|id| {
            resolver::resolve(id)
                .map(ProfileView::from_profile)
                .map_err(map_resolve_error)
        })
        .collect::<application::Result<Vec<ProfileView>>>()?;
    views.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(views)
}

fn matches_filters(profile: &ProfileView, request: &ListRequest) -> bool {
    let vendor_matches = request
        .vendor
        .as_deref()
        .is_none_or(|vendor| contains_ignore_case(&profile.vendor, vendor));
    let source_matches = request.source.is_none_or(|source| {
        profile
            .source
            .eq_ignore_ascii_case(source_filter_label(source))
    });
    let search_matches = request.search.as_deref().is_none_or(|search| {
        contains_ignore_case(&profile.id, search)
            || contains_ignore_case(&profile.vendor, search)
            || contains_ignore_case(&profile.model, search)
    });

    vendor_matches && source_matches && search_matches
}

fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn source_filter_label(filter: ProfileSourceFilter) -> &'static str {
    match filter {
        ProfileSourceFilter::Calibrated => "calibrated",
        ProfileSourceFilter::Synthesized => "synthesized",
        ProfileSourceFilter::Virtual => "virtual",
    }
}

fn map_resolve_error(error: ResolveError) -> CliError {
    match error {
        ResolveError::UnknownProfile(id) => CliError::UnknownProfile(id),
        ResolveError::LoadPack(message) => CliError::LoadProfiles(message),
    }
}

/// Maps a profile's provenance to the catalog's calibration vocabulary.
pub fn source_label(source: &ProfileSource) -> &'static str {
    match source {
        ProfileSource::Upstream => "calibrated",
        ProfileSource::UpstreamDefault => "synthesized",
        ProfileSource::Reference => "virtual",
    }
}

/// A catalog-ready projection shared by typed operations and the CLI adapter.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProfileView {
    pub id: String,
    pub vendor: String,
    pub model: String,
    pub source: String,
    pub paper_width_mm: f64,
    pub printable_width_mm: f64,
    pub printable_width_dots: u32,
    pub dpi_x: u32,
    pub dpi_y: u32,
    pub fonts: FontsView,
    pub features: FeaturesView,
    pub code_page_count: usize,
    pub canonical_profile_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FontsView {
    pub a: FontView,
    pub b: FontView,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FontView {
    pub cell_width_dots: u32,
    pub cell_height_dots: u32,
    pub baseline_dots: u32,
}

impl From<&Font> for FontView {
    fn from(font: &Font) -> Self {
        Self {
            cell_width_dots: font.cell_width_dots,
            cell_height_dots: font.cell_height_dots,
            baseline_dots: font.baseline_dots,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeaturesView {
    pub barcodes: BarcodesView,
    pub graphics: bool,
    pub paper_full_cut: bool,
    pub paper_part_cut: bool,
    pub qr_code: bool,
    pub pulse_standard: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BarcodesView {
    pub function_a: Vec<String>,
    pub function_b: Vec<String>,
}

impl ProfileView {
    pub fn from_profile(profile: &PrinterProfile) -> Self {
        let paper_width_mm = f64::from(profile.paper_width_tenths_mm) / 10.0;
        let printable_width_mm = f64::from(profile.geometry.printable_width_dots)
            / f64::from(profile.geometry.dpi_x)
            * 25.4;

        Self {
            id: profile.id.clone(),
            vendor: profile.vendor.clone(),
            model: profile.model.clone(),
            source: source_label(&profile.source).to_owned(),
            paper_width_mm,
            printable_width_mm,
            printable_width_dots: profile.geometry.printable_width_dots,
            dpi_x: profile.geometry.dpi_x,
            dpi_y: profile.geometry.dpi_y,
            fonts: FontsView {
                a: FontView::from(&profile.fonts.a),
                b: FontView::from(&profile.fonts.b),
            },
            features: FeaturesView {
                barcodes: BarcodesView {
                    function_a: barcode_system_names(&profile.features.barcodes.function_a),
                    function_b: barcode_system_names(&profile.features.barcodes.function_b),
                },
                graphics: profile.features.graphics,
                paper_full_cut: profile.features.paper_full_cut,
                paper_part_cut: profile.features.paper_part_cut,
                qr_code: profile.features.qr_code,
                pulse_standard: profile.features.pulse_standard,
            },
            code_page_count: profile.code_pages.len(),
            canonical_profile_sha256: profile.canonical_profile_sha256.clone(),
        }
    }
}

fn barcode_system_names(systems: &BTreeSet<BarcodeSystem>) -> Vec<String> {
    systems
        .iter()
        .map(|system| match serde_json::to_value(system) {
            Ok(serde_json::Value::String(name)) => name,
            _ => unreachable!("BarcodeSystem always serializes to a JSON string"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_label_maps_every_variant() {
        assert_eq!(source_label(&ProfileSource::Upstream), "calibrated");
        assert_eq!(source_label(&ProfileSource::UpstreamDefault), "synthesized");
        assert_eq!(source_label(&ProfileSource::Reference), "virtual");
    }

    #[test]
    fn list_search_returns_only_matching_profile_views_in_id_order() {
        let response = list(ListRequest {
            vendor: None,
            source: None,
            search: Some("t88".to_owned()),
        })
        .expect("the embedded profile catalog should load");

        let ids: Vec<&str> = response
            .profiles
            .iter()
            .map(|profile| profile.id.as_str())
            .collect();

        assert_eq!(
            ids,
            [
                "TM-T88II",
                "TM-T88III",
                "TM-T88IV",
                "TM-T88IV-SA",
                "TM-T88V",
            ]
        );
        assert!(
            response
                .profiles
                .iter()
                .all(|profile| profile.id.to_lowercase().contains("t88"))
        );
    }

    #[test]
    fn show_returns_the_structured_reference_profile() {
        let response = show(ShowRequest {
            id: "REFERENCE".to_owned(),
        })
        .expect("the reference profile should resolve");

        assert_eq!(response.profile.id, "REFERENCE");
        assert_eq!(response.profile.source, "virtual");
    }
}
