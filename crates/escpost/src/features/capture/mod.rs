//! Typed rendering operation for captured RAW ESC/POS jobs.

use escpost_profiles::PrinterProfile;
use escpost_render::{
    RenderOptions, RenderScale, TracedRenderResult, render_with_trace_and_options,
};

use crate::application::{self, ApplicationError};

pub(crate) mod cli;

/// A completed RAW ESC/POS job, ready to be rendered for the capture viewer.
pub(crate) struct RenderRequest {
    pub(crate) bytes: Vec<u8>,
    pub(crate) profile: &'static PrinterProfile,
    pub(crate) scale: RenderScale,
    pub(crate) antialias: bool,
}

/// Facts produced by rendering one complete captured job.
pub(crate) struct RenderResponse {
    pub(crate) raw_input: Vec<u8>,
    pub(crate) rendered: TracedRenderResult,
}

/// Render an exact RAW job with command tracing for the capture viewer.
pub(crate) fn render_job(request: RenderRequest) -> application::Result<RenderResponse> {
    let options = RenderOptions {
        scale: request.scale,
        antialias: request.antialias,
        ..RenderOptions::default()
    };
    let raw_input = request.bytes;
    let rendered = render_with_trace_and_options(&raw_input, request.profile, &options)
        .map_err(ApplicationError::Render)?;

    Ok(RenderResponse {
        raw_input,
        rendered,
    })
}

#[cfg(test)]
mod tests {
    use escpost_render::RenderScale;

    use super::{RenderRequest, render_job};
    use crate::profiles;

    #[test]
    fn captured_render_preserves_exact_raw_input_and_trace_for_validated_profile() {
        let bytes = b"Captured RAW job\n\x1dV\x00".to_vec();
        let response = render_job(RenderRequest {
            bytes: bytes.clone(),
            profile: profiles::load("REFERENCE").expect("REFERENCE should be available"),
            scale: RenderScale::new(1).unwrap(),
            antialias: false,
        })
        .expect("the reference profile should render a captured RAW job");

        assert_eq!(response.raw_input, bytes);
        assert!(!response.rendered.render.sheets.is_empty());
        assert!(
            response
                .rendered
                .trace
                .sheets
                .iter()
                .any(|sheet| !sheet.commands.is_empty())
        );
    }
}
