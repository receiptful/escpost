use escpost_profiles::PrinterProfile;
use escpost_profiles::resolver::{self, ResolveError};

use crate::application::{self, ApplicationError};

pub(crate) fn load(profile_id: &str) -> application::Result<&'static PrinterProfile> {
    resolver::resolve(profile_id).map_err(|error| match error {
        ResolveError::UnknownProfile(id) => ApplicationError::UnknownProfile(id),
        ResolveError::LoadPack(message) => ApplicationError::LoadProfiles(message),
    })
}

pub(crate) fn available_ids() -> application::Result<Vec<String>> {
    resolver::available_ids().map_err(|error| match error {
        ResolveError::UnknownProfile(id) => ApplicationError::UnknownProfile(id),
        ResolveError::LoadPack(message) => ApplicationError::LoadProfiles(message),
    })
}
