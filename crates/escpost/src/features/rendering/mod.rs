//! Typed rendering operation for already-loaded ESC/POS input.

use escpost_profiles::resolver::{self, ResolveError};
use escpost_render::{
    RenderOptions, RenderResult, RenderScale, Trace, render_with_options,
    render_with_trace_and_options,
};

use crate::application::{self, ApplicationError};

pub(crate) mod cli;

pub(crate) struct Request {
    pub(crate) bytes: Vec<u8>,
    pub(crate) profile_id: String,
    pub(crate) scale: RenderScale,
    pub(crate) antialias: bool,
    pub(crate) trace: bool,
}

pub(crate) struct Response {
    pub(crate) profile_id: String,
    pub(crate) render: RenderResult,
    pub(crate) trace: Option<Trace>,
}

pub(crate) fn render(request: Request) -> application::Result<Response> {
    let profile = resolver::resolve(&request.profile_id).map_err(map_resolve_error)?;
    let options = RenderOptions {
        scale: request.scale,
        antialias: request.antialias,
        ..RenderOptions::default()
    };
    let (render, trace) = if request.trace {
        let traced = render_with_trace_and_options(&request.bytes, profile, &options)
            .map_err(ApplicationError::Render)?;
        (traced.render, Some(traced.trace))
    } else {
        (
            render_with_options(&request.bytes, profile, &options)
                .map_err(ApplicationError::Render)?,
            None,
        )
    };

    Ok(Response {
        profile_id: request.profile_id,
        render,
        trace,
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
    fn untraced_render_returns_sheets_without_a_trace() {
        let response = render(Request {
            bytes: b"A\n".to_vec(),
            profile_id: "REFERENCE".to_owned(),
            scale: RenderScale::new(1).unwrap(),
            antialias: false,
            trace: false,
        })
        .expect("the reference profile should render a minimal receipt");

        assert!(!response.render.sheets.is_empty());
        assert_eq!(response.profile_id, "REFERENCE");
        assert!(response.trace.is_none());
    }

    #[test]
    fn traced_render_returns_the_command_trace() {
        let response = render(Request {
            bytes: b"A\n".to_vec(),
            profile_id: "REFERENCE".to_owned(),
            scale: RenderScale::new(1).unwrap(),
            antialias: false,
            trace: true,
        })
        .expect("the reference profile should trace a minimal receipt");

        assert!(!response.render.sheets.is_empty());
        assert!(response.trace.is_some());
    }
}
