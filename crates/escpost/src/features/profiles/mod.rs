//! Typed operations for the embedded printer-profile catalog.

use std::collections::BTreeSet;

use escpost_profiles::resolver::{self, ResolveError};
use escpost_profiles::{BarcodeSystem, Font, PrinterProfile, ProfileSource};

use crate::application::{self, ApplicationError};

pub(crate) mod cli;

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
    pub(crate) profiles: Vec<ProfileFacts>,
}

pub(crate) struct GetRequest {
    pub(crate) id: String,
}

pub(crate) struct GetResponse {
    pub(crate) profile: ProfileFacts,
}

pub(crate) fn list(request: ListRequest) -> application::Result<ListResponse> {
    let profiles = all_profiles()?
        .into_iter()
        .filter(|profile| matches_filters(profile, &request))
        .collect();
    Ok(ListResponse { profiles })
}

pub(crate) fn get(request: GetRequest) -> application::Result<GetResponse> {
    let profile = resolver::resolve(&request.id)
        .map(ProfileFacts::from_profile)
        .map_err(map_resolve_error)?;
    Ok(GetResponse { profile })
}

fn all_profiles() -> application::Result<Vec<ProfileFacts>> {
    let ids = resolver::available_ids().map_err(map_resolve_error)?;
    let mut views = ids
        .iter()
        .map(|id| {
            resolver::resolve(id)
                .map(ProfileFacts::from_profile)
                .map_err(map_resolve_error)
        })
        .collect::<application::Result<Vec<ProfileFacts>>>()?;
    views.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(views)
}

fn matches_filters(profile: &ProfileFacts, request: &ListRequest) -> bool {
    let vendor_matches = request
        .vendor
        .as_deref()
        .is_none_or(|vendor| contains_ignore_case(&profile.vendor, vendor));
    let source_matches = request
        .source
        .is_none_or(|source| profile.source == source.profile_source());
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

impl ProfileSourceFilter {
    fn profile_source(self) -> ProfileSource {
        match self {
            Self::Calibrated => ProfileSource::Upstream,
            Self::Synthesized => ProfileSource::UpstreamDefault,
            Self::Virtual => ProfileSource::Reference,
        }
    }
}

fn map_resolve_error(error: ResolveError) -> ApplicationError {
    match error {
        ResolveError::UnknownProfile(id) => ApplicationError::UnknownProfile(id),
        ResolveError::LoadPack(message) => ApplicationError::LoadProfiles(message),
    }
}

/// Complete profile information returned by catalog operations.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProfileFacts {
    pub(crate) id: String,
    pub(crate) vendor: String,
    pub(crate) model: String,
    pub(crate) source: ProfileSource,
    pub(crate) paper_width_mm: f64,
    pub(crate) printable_width_mm: f64,
    pub(crate) printable_width_dots: u32,
    pub(crate) dpi_x: u32,
    pub(crate) dpi_y: u32,
    pub(crate) fonts: FontsFacts,
    pub(crate) features: FeaturesFacts,
    pub(crate) code_page_count: usize,
    pub(crate) canonical_profile_sha256: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FontsFacts {
    pub(crate) a: FontFacts,
    pub(crate) b: FontFacts,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FontFacts {
    pub(crate) cell_width_dots: u32,
    pub(crate) cell_height_dots: u32,
    pub(crate) baseline_dots: u32,
}

impl From<&Font> for FontFacts {
    fn from(font: &Font) -> Self {
        Self {
            cell_width_dots: font.cell_width_dots,
            cell_height_dots: font.cell_height_dots,
            baseline_dots: font.baseline_dots,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FeaturesFacts {
    pub(crate) barcodes: BarcodeFacts,
    pub(crate) graphics: bool,
    pub(crate) paper_full_cut: bool,
    pub(crate) paper_part_cut: bool,
    pub(crate) qr_code: bool,
    pub(crate) pulse_standard: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BarcodeFacts {
    pub(crate) function_a: BTreeSet<BarcodeSystem>,
    pub(crate) function_b: BTreeSet<BarcodeSystem>,
}

impl ProfileFacts {
    fn from_profile(profile: &PrinterProfile) -> Self {
        let paper_width_mm = f64::from(profile.paper_width_tenths_mm) / 10.0;
        let printable_width_mm = f64::from(profile.geometry.printable_width_dots)
            / f64::from(profile.geometry.dpi_x)
            * 25.4;

        Self {
            id: profile.id.clone(),
            vendor: profile.vendor.clone(),
            model: profile.model.clone(),
            source: profile.source.clone(),
            paper_width_mm,
            printable_width_mm,
            printable_width_dots: profile.geometry.printable_width_dots,
            dpi_x: profile.geometry.dpi_x,
            dpi_y: profile.geometry.dpi_y,
            fonts: FontsFacts {
                a: FontFacts::from(&profile.fonts.a),
                b: FontFacts::from(&profile.fonts.b),
            },
            features: FeaturesFacts {
                barcodes: BarcodeFacts {
                    function_a: profile.features.barcodes.function_a.clone(),
                    function_b: profile.features.barcodes.function_b.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn get_returns_the_structured_reference_profile() {
        let response = get(GetRequest {
            id: "REFERENCE".to_owned(),
        })
        .expect("the reference profile should resolve");

        assert_eq!(response.profile.id, "REFERENCE");
        assert_eq!(response.profile.source, ProfileSource::Reference);
    }

    #[test]
    fn get_keeps_profile_provenance_and_barcode_support_as_typed_facts() {
        let response = get(GetRequest {
            id: "TM-T88III".to_owned(),
        })
        .expect("the synthesized fixture profile should resolve");

        assert_eq!(response.profile.source, ProfileSource::UpstreamDefault);
        assert!(
            response
                .profile
                .features
                .barcodes
                .function_b
                .contains(&BarcodeSystem::Code128)
        );
    }
}
