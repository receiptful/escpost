//! Typed rendering operation for already-loaded ESC/POS input.

use escpost_profiles::resolver::{self, ResolveError};
use escpost_render::{RenderOptions, RenderResult, RenderScale, render_with_options};

use crate::application::{self, ApplicationError};

pub(crate) mod cli;

pub(crate) struct Request {
    pub(crate) bytes: Vec<u8>,
    pub(crate) profile_id: String,
    pub(crate) scale: RenderScale,
    pub(crate) antialias: bool,
}

pub(crate) struct Response {
    pub(crate) profile_id: String,
    pub(crate) render: RenderResult,
}

pub(crate) fn render(request: Request) -> application::Result<Response> {
    let profile = resolver::resolve(&request.profile_id).map_err(map_resolve_error)?;
    let options = RenderOptions {
        scale: request.scale,
        antialias: request.antialias,
        ..RenderOptions::default()
    };
    let render =
        render_with_options(&request.bytes, profile, &options).map_err(ApplicationError::Render)?;

    Ok(Response {
        profile_id: request.profile_id,
        render,
    })
}

fn map_resolve_error(error: ResolveError) -> ApplicationError {
    match error {
        ResolveError::UnknownProfile(id) => ApplicationError::UnknownProfile(id),
        ResolveError::LoadPack(message) => ApplicationError::LoadProfiles(message),
    }
}

#[cfg(test)]
mod tests {
    use escpost_render::RenderScale;

    use super::{Request, render};

    #[test]
    fn render_returns_sheets_for_the_resolved_profile() {
        let response = render(Request {
            bytes: b"A\n".to_vec(),
            profile_id: "REFERENCE".to_owned(),
            scale: RenderScale::new(1).unwrap(),
            antialias: false,
        })
        .expect("the reference profile should render a minimal receipt");

        assert!(!response.render.sheets.is_empty());
        assert_eq!(response.profile_id, "REFERENCE");
    }
}
