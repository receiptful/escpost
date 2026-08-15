use escpost_profiles::PrinterProfile;
use escpost_profiles::resolver::{self, ResolveError};

use crate::error::CliError;

pub(crate) fn load(profile_id: &str) -> Result<&'static PrinterProfile, CliError> {
    resolver::resolve(profile_id).map_err(|error| match error {
        ResolveError::UnknownProfile(id) => CliError::UnknownProfile(id),
        ResolveError::LoadPack(message) => CliError::LoadProfiles(message),
    })
}

pub(crate) fn available_ids() -> Result<Vec<String>, CliError> {
    resolver::available_ids().map_err(|error| match error {
        ResolveError::UnknownProfile(id) => CliError::UnknownProfile(id),
        ResolveError::LoadPack(message) => CliError::LoadProfiles(message),
    })
}
