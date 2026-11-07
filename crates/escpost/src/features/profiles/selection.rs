use std::fmt;

use super::ProfileFacts;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProfileChoice {
    profile_id: Option<String>,
    vendor: String,
    model: String,
}

impl ProfileChoice {
    pub(crate) fn from_profile(profile: &ProfileFacts) -> Self {
        Self {
            profile_id: Some(profile.id.clone()),
            vendor: profile.vendor.clone(),
            model: profile.model.clone(),
        }
    }

    pub(crate) fn no_profile() -> Self {
        Self {
            profile_id: None,
            vendor: String::new(),
            model: String::new(),
        }
    }

    pub(crate) fn into_profile_id(self) -> Option<String> {
        self.profile_id
    }
}

impl fmt::Display for ProfileChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.profile_id {
            Some(profile_id) => write!(
                formatter,
                "{} · {} — {}",
                self.vendor, self.model, profile_id
            ),
            None => formatter.write_str("No profile"),
        }
    }
}

pub(crate) fn profile_choices(profiles: &[ProfileFacts]) -> Vec<ProfileChoice> {
    let mut choices: Vec<ProfileChoice> =
        profiles.iter().map(ProfileChoice::from_profile).collect();
    choices.sort_by_cached_key(|choice| {
        (
            choice.vendor.to_lowercase(),
            choice.model.to_lowercase(),
            choice
                .profile_id
                .as_deref()
                .unwrap_or_default()
                .to_lowercase(),
        )
    });
    choices
}

pub(crate) fn optional_profile_choices(profiles: &[ProfileFacts]) -> Vec<ProfileChoice> {
    let mut choices = vec![ProfileChoice::no_profile()];
    choices.extend(profile_choices(profiles));
    choices
}

pub(crate) fn substring_scorer(
    query: &str,
    _choice: &ProfileChoice,
    rendered_choice: &str,
    index: usize,
) -> Option<i64> {
    rendered_choice
        .to_lowercase()
        .contains(&query.to_lowercase())
        .then(|| i64::try_from(index).ok().map(|index| -index))
        .flatten()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use escpost_profiles::ProfileSource;

    use super::{optional_profile_choices, profile_choices, substring_scorer};
    use crate::features::profiles::{
        BarcodeFacts, FeaturesFacts, FontFacts, FontsFacts, ProfileFacts,
    };

    fn profile(id: &str, vendor: &str, model: &str) -> ProfileFacts {
        ProfileFacts {
            id: id.to_owned(),
            vendor: vendor.to_owned(),
            model: model.to_owned(),
            source: ProfileSource::Upstream,
            paper_width_mm: 80.0,
            printable_width_mm: 72.0,
            printable_width_dots: 576,
            dpi_x: 203,
            dpi_y: 203,
            fonts: FontsFacts {
                a: FontFacts {
                    cell_width_dots: 12,
                    cell_height_dots: 24,
                    baseline_dots: 21,
                },
                b: FontFacts {
                    cell_width_dots: 9,
                    cell_height_dots: 17,
                    baseline_dots: 15,
                },
            },
            features: FeaturesFacts {
                barcodes: BarcodeFacts {
                    function_a: BTreeSet::new(),
                    function_b: BTreeSet::new(),
                },
                graphics: true,
                paper_full_cut: true,
                paper_part_cut: false,
                qr_code: true,
                pulse_standard: true,
            },
            code_page_count: 1,
            canonical_profile_sha256: "fixture".to_owned(),
        }
    }

    fn profiles() -> Vec<ProfileFacts> {
        vec![
            profile("zeta", "Epson", "TM-T88"),
            profile("BROTHER", "Brother", "QL-800"),
            profile("ALPHA", "epson", "tm-t88"),
            profile("OMEGA", "Epson", "TM-T90"),
        ]
    }

    #[test]
    fn choices_sort_by_vendor_model_then_profile_id_case_insensitively() {
        let ids: Vec<Option<String>> = profile_choices(&profiles())
            .into_iter()
            .map(|choice| choice.into_profile_id())
            .collect();

        assert_eq!(
            ids,
            [
                Some("BROTHER".to_owned()),
                Some("ALPHA".to_owned()),
                Some("zeta".to_owned()),
                Some("OMEGA".to_owned()),
            ]
        );
    }

    #[test]
    fn choice_labels_put_the_profile_id_last() {
        let labels: Vec<String> = profile_choices(&profiles())
            .iter()
            .map(ToString::to_string)
            .collect();

        assert_eq!(
            labels,
            [
                "Brother · QL-800 — BROTHER",
                "epson · tm-t88 — ALPHA",
                "Epson · TM-T88 — zeta",
                "Epson · TM-T90 — OMEGA",
            ]
        );
    }

    #[test]
    fn substring_scorer_matches_complete_vendor_model_and_id_queries_in_presentation_order() {
        let choices = profile_choices(&profiles());

        let matching_ids = |query: &str| {
            let mut matches: Vec<(i64, String)> = choices
                .iter()
                .enumerate()
                .filter_map(|(index, choice)| {
                    substring_scorer(query, choice, &choice.to_string(), index)
                        .map(|score| (score, choice.clone().into_profile_id().unwrap()))
                })
                .collect();
            matches.sort_by_key(|entry| std::cmp::Reverse(entry.0));
            matches.into_iter().map(|(_, id)| id).collect::<Vec<_>>()
        };

        assert_eq!(matching_ids("Epson"), ["ALPHA", "zeta", "OMEGA"]);
        assert_eq!(matching_ids("TM-T88"), ["ALPHA", "zeta"]);
        assert_eq!(matching_ids("ZETA"), ["zeta"]);
    }

    #[test]
    fn optional_choices_put_exactly_one_no_profile_choice_first() {
        let choices = optional_profile_choices(&profiles());
        let labels: Vec<String> = choices.iter().map(ToString::to_string).collect();

        assert_eq!(labels[0], "No profile");
        assert_eq!(
            labels.iter().filter(|label| *label == "No profile").count(),
            1
        );
        assert_eq!(
            &labels[1..],
            [
                "Brother · QL-800 — BROTHER",
                "epson · tm-t88 — ALPHA",
                "Epson · TM-T88 — zeta",
                "Epson · TM-T90 — OMEGA",
            ]
        );
        assert_eq!(choices.into_iter().next().unwrap().into_profile_id(), None);
    }
}
