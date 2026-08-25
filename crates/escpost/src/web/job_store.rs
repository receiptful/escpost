use std::collections::VecDeque;
use std::sync::Arc;

use escpost_render::TracedRenderResult;
use tokio::sync::{RwLock, watch};

use super::{CommandResponse, RenderResponse, SheetResponse, command_responses, epoch_millis};

#[derive(Clone)]
pub(crate) struct JobStore {
    pub(super) state: Arc<RwLock<JobStoreState>>,
    runtime_status: watch::Sender<JobRuntimeStatus>,
}

pub(super) struct JobStoreState {
    pub(super) jobs: VecDeque<Arc<RenderedJob>>,
    pub(super) error: Option<String>,
    pub(super) generation: u64,
    pub(super) waiting_hint: Option<String>,
    pub(super) completion: Option<&'static str>,
    pub(super) receiving: usize,
    /// Profile the server renders with, shown before the first job arrives.
    pub(super) session_profile: String,
    /// Whether renders are anti-aliased grayscale previews. The viewer smooths
    /// those and keeps the faithful 1-bit dots crisp.
    pub(super) antialias: bool,
    /// Wall-clock time the current job completed, in Unix epoch milliseconds.
    pub(super) completed_at: Option<u64>,
    /// The current job's exact captured bytes, offered for download.
    pub(super) raw_input: Option<Vec<u8>>,
    /// Captured RAW jobs that successfully rendered during this server session.
    jobs_processed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct JobRuntimeStatus {
    pub(crate) receiving: bool,
    pub(crate) jobs_processed: u64,
}

pub(super) struct RenderedJob {
    pub(super) profile: String,
    /// Non-fatal render diagnostics, pre-formatted for display.
    pub(super) warnings: Vec<String>,
    pub(super) sheets: Vec<RenderedWebSheet>,
    pub(super) trace_sheets: Vec<TraceWebSheet>,
}

pub(super) struct RenderedWebSheet {
    pub(super) name: String,
    pub(super) width_dots: u32,
    pub(super) height_dots: u32,
    pub(super) png: Vec<u8>,
}

pub(super) struct TraceWebSheet {
    pub(super) commands: Vec<CommandResponse>,
}

impl JobStore {
    pub(crate) fn with_render(rendered: TracedRenderResult, antialias: bool) -> Self {
        let runtime_status = JobRuntimeStatus {
            receiving: false,
            jobs_processed: 0,
        };
        let (runtime_status, _) = watch::channel(runtime_status);
        Self {
            state: Arc::new(RwLock::new(JobStoreState {
                jobs: VecDeque::from([Arc::new(RenderedJob::from(rendered))]),
                error: None,
                generation: 1,
                waiting_hint: None,
                completion: None,
                receiving: 0,
                session_profile: String::new(),
                antialias,
                completed_at: Some(epoch_millis()),
                raw_input: None,
                jobs_processed: 0,
            })),
            runtime_status,
        }
    }

    /// Create a store with no job yet. The web viewer shows `hint` and the
    /// `profile` until the first job arrives, which suits a listener that
    /// renders on demand with a known profile.
    pub(crate) fn awaiting_jobs(profile: String, hint: String, antialias: bool) -> Self {
        let runtime_status = JobRuntimeStatus {
            receiving: false,
            jobs_processed: 0,
        };
        let (runtime_status, _) = watch::channel(runtime_status);
        Self {
            state: Arc::new(RwLock::new(JobStoreState {
                jobs: VecDeque::new(),
                error: None,
                generation: 0,
                waiting_hint: Some(hint),
                completion: None,
                receiving: 0,
                session_profile: profile,
                antialias,
                completed_at: None,
                raw_input: None,
                jobs_processed: 0,
            })),
            runtime_status,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn subscribe_runtime_status(&self) -> watch::Receiver<JobRuntimeStatus> {
        self.runtime_status.subscribe()
    }

    pub(crate) fn runtime_status(&self) -> JobRuntimeStatus {
        self.runtime_status.borrow().clone()
    }

    fn publish_runtime_status(&self, state: &JobStoreState) {
        let next = JobRuntimeStatus {
            receiving: state.receiving > 0,
            jobs_processed: state.jobs_processed,
        };
        let _ = self.runtime_status.send_if_modified(|current| {
            if *current == next {
                return false;
            }
            *current = next;
            true
        });
    }

    /// Mark that a connection has started sending a job. The viewer reports this
    /// until the matching `end_capture`.
    pub(crate) async fn begin_capture(&self) {
        let mut state = self.state.write().await;
        state.receiving += 1;
        self.publish_runtime_status(&state);
    }

    /// Mark that an in-progress job has finished sending (completed or dropped).
    pub(crate) async fn end_capture(&self) {
        let mut state = self.state.write().await;
        state.receiving = state.receiving.saturating_sub(1);
        self.publish_runtime_status(&state);
    }

    /// Replace the preview with a render that has no capture semantics, such as
    /// `render --web`. Its source is a file, so nothing is offered for download.
    pub(crate) async fn replace_render(&self, rendered: TracedRenderResult) {
        self.store_render(rendered, None, None, false).await;
    }

    /// Replace the preview with a captured job, recording how it ended so the
    /// viewer can distinguish a closed connection from an idle timeout, and
    /// keeping its exact bytes for download.
    pub(crate) async fn replace_captured(
        &self,
        rendered: TracedRenderResult,
        completion: &'static str,
        raw_input: Vec<u8>,
    ) {
        self.store_render(rendered, Some(completion), Some(raw_input), true)
            .await;
    }

    async fn store_render(
        &self,
        rendered: TracedRenderResult,
        completion: Option<&'static str>,
        raw_input: Option<Vec<u8>>,
        captured: bool,
    ) {
        let mut state = self.state.write().await;
        state.jobs = VecDeque::from([Arc::new(RenderedJob::from(rendered))]);
        state.error = None;
        state.generation += 1;
        state.completion = completion;
        state.completed_at = Some(epoch_millis());
        state.raw_input = raw_input;
        if captured {
            state.jobs_processed += 1;
            self.publish_runtime_status(&state);
        }
    }

    /// The current job's captured bytes and completion time, for download.
    pub(super) async fn raw_input_download(&self) -> Option<(Vec<u8>, u64)> {
        let state = self.state.read().await;
        let bytes = state.raw_input.clone()?;
        Some((bytes, state.completed_at.unwrap_or(0)))
    }

    pub(crate) async fn set_error(&self, error: String) {
        self.state.write().await.error = Some(error);
    }

    pub(super) async fn snapshot(&self) -> Option<(Arc<RenderedJob>, u64, Option<String>)> {
        let state = self.state.read().await;
        state
            .jobs
            .front()
            .cloned()
            .map(|job| (job, state.generation, state.error.clone()))
    }

    pub(super) async fn render_response(&self) -> RenderResponse {
        let state = self.state.read().await;
        let Some(job) = state.jobs.front() else {
            // No job yet: report a waiting state so the viewer can guide the
            // developer rather than showing a bare error.
            return RenderResponse {
                profile: state.session_profile.clone(),
                generation: state.generation,
                error: state.error.clone(),
                hint: state.waiting_hint.clone(),
                completion: None,
                receiving: state.receiving > 0,
                completed_at: None,
                input_available: false,
                antialias: state.antialias,
                warnings: Vec::new(),
                sheets: Vec::new(),
            };
        };
        let sheets = job
            .trace_sheets
            .iter()
            .enumerate()
            .map(|(index, trace_sheet)| {
                let rendered = job.sheets.get(index);
                SheetResponse {
                    name: rendered
                        .map(|sheet| sheet.name.clone())
                        .unwrap_or_else(|| format!("sheet-{:03}", index + 1)),
                    order: index + 1,
                    width_dots: rendered.map(|sheet| sheet.width_dots),
                    height_dots: rendered.map(|sheet| sheet.height_dots),
                    url: rendered.map(|_| format!("/sheets/{}.png", index + 1)),
                    commands: trace_sheet.commands.clone(),
                }
            })
            .collect();
        RenderResponse {
            profile: job.profile.clone(),
            generation: state.generation,
            error: state.error.clone(),
            hint: None,
            completion: state.completion,
            receiving: state.receiving > 0,
            completed_at: state.completed_at,
            input_available: state.raw_input.is_some(),
            antialias: state.antialias,
            warnings: job.warnings.clone(),
            sheets,
        }
    }
}

impl From<TracedRenderResult> for RenderedJob {
    fn from(traced: TracedRenderResult) -> Self {
        let TracedRenderResult { render, trace } = traced;
        Self {
            profile: render.metadata.profile_id,
            warnings: render.warnings.iter().map(ToString::to_string).collect(),
            sheets: render
                .sheets
                .into_iter()
                .enumerate()
                .map(|(index, sheet)| RenderedWebSheet {
                    name: format!("sheet-{:03}.png", index + 1),
                    width_dots: sheet.surface.width(),
                    height_dots: sheet.surface.height(),
                    png: sheet.png,
                })
                .collect(),
            trace_sheets: trace
                .sheets
                .into_iter()
                .map(|sheet| TraceWebSheet {
                    commands: command_responses(sheet.commands),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{JobRuntimeStatus, JobStore};

    #[tokio::test]
    async fn runtime_subscriber_starts_current_and_receives_public_changes() {
        let store = JobStore::awaiting_jobs("REFERENCE".into(), "Waiting".into(), false);
        let mut status = store.subscribe_runtime_status();

        assert_eq!(
            status.borrow().clone(),
            JobRuntimeStatus {
                receiving: false,
                jobs_processed: 0,
            }
        );

        store.begin_capture().await;
        status
            .changed()
            .await
            .expect("the store should remain open");
        assert!(status.borrow().receiving);

        store.end_capture().await;
        status
            .changed()
            .await
            .expect("the store should remain open");
        assert!(!status.borrow().receiving);
    }

    #[tokio::test]
    async fn runtime_subscribers_are_independent_and_may_disconnect() {
        let store = JobStore::awaiting_jobs("REFERENCE".into(), "Waiting".into(), false);
        let dropped = store.subscribe_runtime_status();
        let mut remaining = store.subscribe_runtime_status();
        drop(dropped);

        store.begin_capture().await;
        remaining
            .changed()
            .await
            .expect("the store should remain open");

        assert!(remaining.borrow().receiving);
        store.end_capture().await;
    }

    #[tokio::test]
    async fn concurrent_capture_does_not_publish_when_receiving_stays_true() {
        let store = JobStore::awaiting_jobs("REFERENCE".into(), "Waiting".into(), false);
        let mut status = store.subscribe_runtime_status();
        let _ = status.borrow_and_update();

        store.begin_capture().await;
        status
            .changed()
            .await
            .expect("the store should remain open");
        assert!(status.borrow_and_update().receiving);

        store.begin_capture().await;
        assert!(
            !status.has_changed().expect("the store should remain open"),
            "the public status has not changed"
        );

        store.end_capture().await;
        assert!(
            !status.has_changed().expect("the store should remain open"),
            "one capture is still active"
        );

        store.end_capture().await;
        status
            .changed()
            .await
            .expect("the store should remain open");
        assert!(!status.borrow_and_update().receiving);
    }
}
