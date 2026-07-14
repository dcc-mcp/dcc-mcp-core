//! Adapters that bridge `dcc-mcp-http` internal registries to the
//! `dcc-mcp-skill-rest` provider traits (#818 phase 2 bridge).
//!
//! These adapters are wired into [`SkillRestService`] inside
//! [`super::server`] so that `GET /v1/resources` and `GET /v1/prompts`
//! return real data instead of the default empty responses.
//!
//! Each adapter satisfies the DIP boundary in `dcc-mcp-skill-rest`:
//! the REST layer depends on the trait, not on these concrete types.

use std::sync::Arc;

use dcc_mcp_job::job::{Job, JobManager, JobStatus};
use dcc_mcp_skill_rest::{
    CallOutcome, EventStream, JobController, JobEvent, PromptArgumentSpec, PromptContent,
    PromptGetResponse, PromptListEntry, PromptMessage, PromptProvider, ResourceContent,
    ResourceListEntry, ResourceProvider, ResourceReadResponse, ServiceError, ServiceErrorKind,
    ToolSlug,
};
use dcc_mcp_skills::SkillCatalog;

// ── JobManagerAdapter ──────────────────────────────────────────────────────

/// Bridges the server's shared [`JobManager`] to REST job events and cancel.
pub(crate) struct JobManagerAdapter {
    manager: Arc<JobManager>,
    events: tokio::sync::broadcast::Sender<dcc_mcp_job::job::JobEvent>,
}

impl JobManagerAdapter {
    pub(crate) fn new(manager: Arc<JobManager>) -> Self {
        let (events, _) = tokio::sync::broadcast::channel(128);
        let event_sink = events.clone();
        manager.subscribe(move |event| {
            let _ = event_sink.send(event);
        });
        Self { manager, events }
    }

    fn rest_event(job: &Job) -> JobEvent {
        match job.status {
            JobStatus::Completed => JobEvent::Done {
                result: CallOutcome {
                    slug: ToolSlug(job.tool_name.clone()),
                    output: job.result.clone().unwrap_or(serde_json::Value::Null),
                    validation_skipped: false,
                },
            },
            JobStatus::Failed | JobStatus::Cancelled | JobStatus::Interrupted => JobEvent::Error {
                error: ServiceError::new(
                    ServiceErrorKind::Internal,
                    job.error
                        .clone()
                        .unwrap_or_else(|| format!("job ended with status {:?}", job.status)),
                ),
            },
            JobStatus::Pending | JobStatus::Running => {
                let (progress, total, message) =
                    job.progress.as_ref().map_or((None, None, None), |value| {
                        let total = value.total as f64;
                        let ratio = (value.total > 0).then_some(value.current as f64 / total);
                        (ratio, Some(total), value.message.clone())
                    });
                JobEvent::Progress {
                    progress,
                    total,
                    message,
                }
            }
        }
    }
}

impl JobController for JobManagerAdapter {
    fn subscribe(&self, job_id: &str) -> Result<EventStream, ServiceError> {
        use futures::{StreamExt, stream};

        let handle = self.manager.get(job_id).ok_or_else(|| {
            ServiceError::new(
                ServiceErrorKind::NotFound,
                format!("job not found: {job_id}"),
            )
        })?;
        let initial = {
            let job = handle.read();
            (Self::rest_event(&job), job.status.is_terminal())
        };
        if initial.1 {
            return Ok(Box::pin(stream::once(async move { Ok(initial.0) })));
        }

        let manager = Arc::clone(&self.manager);
        let target_id = job_id.to_string();
        let receiver = self.events.subscribe();
        let updates = stream::unfold((receiver, false), move |(mut receiver, done)| {
            let manager = Arc::clone(&manager);
            let target_id = target_id.clone();
            async move {
                if done {
                    return None;
                }
                loop {
                    match receiver.recv().await {
                        Ok(event) if event.id == target_id => {
                            let handle = manager.get(&target_id)?;
                            let job = handle.read();
                            let terminal = job.status.is_terminal();
                            let event = Self::rest_event(&job);
                            return Some((Ok(event), (receiver, terminal)));
                        }
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                    }
                }
            }
        });
        Ok(Box::pin(
            stream::once(async move { Ok(initial.0) }).chain(updates),
        ))
    }

    fn cancel(&self, job_id: &str) -> Result<(), ServiceError> {
        self.manager.cancel(job_id).ok_or_else(|| {
            ServiceError::new(
                ServiceErrorKind::NotFound,
                format!("running job not found: {job_id}"),
            )
        })
    }
}

// ── ResourceRegistryAdapter ───────────────────────────────────────────────

/// Bridges [`crate::resources::ResourceRegistry`] to
/// [`ResourceProvider`].
pub(crate) struct ResourceRegistryAdapter {
    registry: crate::resources::ResourceRegistry,
    catalog: Arc<SkillCatalog>,
}

impl ResourceRegistryAdapter {
    pub(crate) fn new(
        registry: crate::resources::ResourceRegistry,
        catalog: Arc<SkillCatalog>,
    ) -> Self {
        Self { registry, catalog }
    }

    fn sync(&self) {
        let catalog = self.catalog.clone();
        self.registry
            .sync_skill_resources(|visit| catalog.for_each_loaded_metadata(|md| visit(md)));
    }
}

impl ResourceProvider for ResourceRegistryAdapter {
    fn list(&self) -> Vec<ResourceListEntry> {
        self.sync();
        self.registry
            .list()
            .into_iter()
            .map(|r| ResourceListEntry {
                uri: r.uri,
                name: r.name,
                description: r.description,
                mime_type: r.mime_type,
            })
            .collect()
    }

    fn read(&self, uri: &str) -> Result<ResourceReadResponse, ServiceError> {
        self.sync();
        self.registry
            .read(uri)
            .map_err(|e| match e {
                crate::resources::ResourceError::NotFound(msg)
                | crate::resources::ResourceError::NotEnabled(msg) => {
                    ServiceError::new(ServiceErrorKind::NotFound, msg)
                }
                crate::resources::ResourceError::Read(msg) => {
                    ServiceError::new(ServiceErrorKind::Internal, msg)
                }
            })
            .map(|result| ResourceReadResponse {
                contents: result
                    .contents
                    .into_iter()
                    .map(|c| ResourceContent {
                        uri: c.uri,
                        mime_type: c.mime_type,
                        text: c.text,
                        blob: c.blob,
                    })
                    .collect(),
            })
    }
}

// ── PromptRegistryAdapter ─────────────────────────────────────────────────

/// Bridges [`crate::prompts::PromptRegistry`] to [`PromptProvider`].
pub(crate) struct PromptRegistryAdapter {
    registry: crate::prompts::PromptRegistry,
    catalog: Arc<SkillCatalog>,
}

impl PromptRegistryAdapter {
    pub(crate) fn new(
        registry: crate::prompts::PromptRegistry,
        catalog: Arc<SkillCatalog>,
    ) -> Self {
        Self { registry, catalog }
    }
}

impl PromptProvider for PromptRegistryAdapter {
    fn list(&self) -> Vec<PromptListEntry> {
        let catalog = self.catalog.clone();
        self.registry
            .list(|visit| catalog.for_each_loaded_metadata(|md| visit(md)))
            .into_iter()
            .map(|p| PromptListEntry {
                name: p.name,
                description: p.description,
                arguments: p
                    .arguments
                    .into_iter()
                    .map(|a| PromptArgumentSpec {
                        name: a.name,
                        description: a.description,
                        required: a.required,
                    })
                    .collect(),
                meta: p.meta,
            })
            .collect()
    }

    fn get(
        &self,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<PromptGetResponse, ServiceError> {
        // Convert JSON arguments Value into HashMap<String, String>
        let args: std::collections::HashMap<String, String> = arguments
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        let catalog = self.catalog.clone();
        self.registry
            .get(name, &args, |visit| {
                catalog.for_each_loaded_metadata(|md| visit(md))
            })
            .map_err(|e| match e {
                crate::prompts::PromptError::NotFound(msg) => {
                    ServiceError::new(ServiceErrorKind::NotFound, msg)
                }
                crate::prompts::PromptError::MissingArg(arg) => ServiceError::new(
                    ServiceErrorKind::InvalidParams,
                    format!("missing required argument: {arg}"),
                ),
                crate::prompts::PromptError::Load(msg) => {
                    ServiceError::new(ServiceErrorKind::Internal, msg)
                }
            })
            .map(|result| PromptGetResponse {
                description: result.description,
                messages: result
                    .messages
                    .into_iter()
                    .map(|m| PromptMessage {
                        role: m.role,
                        content: match m.content {
                            dcc_mcp_jsonrpc::McpPromptContent::Text { text } => {
                                PromptContent::Text { text }
                            }
                        },
                    })
                    .collect(),
            })
    }

    fn diagnostics(&self) -> Option<serde_json::Value> {
        let catalog = self.catalog.clone();
        serde_json::to_value(
            self.registry
                .diagnostics(|visit| catalog.for_each_loaded_metadata(|md| visit(md))),
        )
        .ok()
    }
}

#[cfg(test)]
mod job_tests {
    use futures::StreamExt;
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn job_adapter_streams_pending_then_terminal_result() {
        let manager = Arc::new(JobManager::new());
        let adapter = JobManagerAdapter::new(Arc::clone(&manager));
        let handle = manager.create("nuke.layered_compositing.render");
        let job_id = handle.read().id.clone();
        let mut stream = adapter.subscribe(&job_id).expect("subscribe");

        assert!(matches!(
            stream.next().await,
            Some(Ok(JobEvent::Progress { .. }))
        ));
        manager.start(&job_id).expect("start");
        manager
            .complete(&job_id, json!({"frames": 1}))
            .expect("complete");

        let event = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .expect("terminal event")
            .expect("stream item")
            .expect("event");
        assert!(matches!(
            event,
            JobEvent::Done { result } if result.output == json!({"frames": 1})
        ));
    }
}
