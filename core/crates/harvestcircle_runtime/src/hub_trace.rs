//! Protected read-only trace and output commands; no file/printer or stock effects.
use super::*;
use harvestcircle_application::hub_output::{
    LocalOutput, LocalOutputError, identification_output, trace_output,
};
use harvestcircle_domain::hub::reference::HubRecordId;
use harvestcircle_domain::hub::trace::TraceSnapshot;
use harvestcircle_storage::{
    Database, HubTraceHistoryPage, HubTraceHistoryQuery, HubTracePage, HubTraceQuery,
    HubTraceSnapshotQuery,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HubLocalOutputRequest {
    Identification {
        preparation: HubRecordId,
        identification: HubRecordId,
    },
    /// Current-population output must bind the snapshot explicitly reviewed.
    Trace(HubTraceSnapshotQuery),
}
impl HubLocalOutputRequest {
    pub fn validate(&self) -> Result<(), SafeError> {
        if let Self::Trace(query) = self {
            query.validate()?;
            if query.expected_revision.is_none() || query.expected_catalog_revision.is_none() {
                return Err(output_error(LocalOutputError::InvalidSnapshot));
            }
        }
        Ok(())
    }
}
pub(super) enum HubTraceCommand {
    Page(HubTraceQuery),
    Snapshot(HubTraceSnapshotQuery),
    History(HubTraceHistoryQuery),
    Output(HubLocalOutputRequest),
}
pub(super) enum HubTraceValue {
    Page(Box<HubTracePage>),
    Snapshot(Box<TraceSnapshot>),
    History(Box<HubTraceHistoryPage>),
    Output(LocalOutput),
}

impl RuntimeActorHandle {
    pub async fn hub_trace_page(&self, query: HubTraceQuery) -> Result<HubTracePage, SafeError> {
        query.validate()?;
        match self
            .dispatch_handling(HubHandlingCommand::Trace(Box::new(HubTraceCommand::Page(
                query,
            ))))
            .await?
        {
            HubHandlingValue::Trace(HubTraceValue::Page(value)) => Ok(*value),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_trace_snapshot(
        &self,
        query: HubTraceSnapshotQuery,
    ) -> Result<TraceSnapshot, SafeError> {
        query.validate()?;
        match self
            .dispatch_handling(HubHandlingCommand::Trace(Box::new(
                HubTraceCommand::Snapshot(query),
            )))
            .await?
        {
            HubHandlingValue::Trace(HubTraceValue::Snapshot(value)) => Ok(*value),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_trace_history(
        &self,
        query: HubTraceHistoryQuery,
    ) -> Result<HubTraceHistoryPage, SafeError> {
        query.validate()?;
        match self
            .dispatch_handling(HubHandlingCommand::Trace(Box::new(
                HubTraceCommand::History(query),
            )))
            .await?
        {
            HubHandlingValue::Trace(HubTraceValue::History(value)) => Ok(*value),
            _ => Err(invalid_actor_response()),
        }
    }
    pub async fn hub_local_output(
        &self,
        request: HubLocalOutputRequest,
    ) -> Result<LocalOutput, SafeError> {
        request.validate()?;
        match self
            .dispatch_handling(HubHandlingCommand::Trace(Box::new(
                HubTraceCommand::Output(request),
            )))
            .await?
        {
            HubHandlingValue::Trace(HubTraceValue::Output(value)) => Ok(value),
            _ => Err(invalid_actor_response()),
        }
    }
}
pub(super) async fn execute(
    database: &Database,
    actor: PublicKey,
    command: HubTraceCommand,
) -> Result<HubTraceValue, SafeError> {
    match command {
        HubTraceCommand::Page(query) => Ok(HubTraceValue::Page(Box::new(
            database.hub_trace_page(actor, query).await?,
        ))),
        HubTraceCommand::Snapshot(query) => Ok(HubTraceValue::Snapshot(Box::new(
            database.hub_trace_snapshot(actor, query).await?,
        ))),
        HubTraceCommand::History(query) => Ok(HubTraceValue::History(Box::new(
            database.hub_trace_history(actor, query).await?,
        ))),
        HubTraceCommand::Output(request) => {
            request.validate()?;
            let output = match request {
                HubLocalOutputRequest::Identification {
                    preparation,
                    identification,
                } => {
                    let view = database.hub_dispatch_record(actor, preparation).await?;
                    identification_output(&view.record, identification)
                }
                HubLocalOutputRequest::Trace(query) => {
                    let snapshot = database.hub_trace_snapshot(actor, query).await?;
                    trace_output(&snapshot)
                }
            }
            .map_err(output_error)?;
            Ok(HubTraceValue::Output(output))
        }
    }
}
fn output_error(error: LocalOutputError) -> SafeError {
    let (code, message) = match error {
        LocalOutputError::Capacity => (
            SafeErrorCode::HubCapacity,
            "This population exceeds the bounded local document size. Inspect its records; no physical action was submitted.",
        ),
        LocalOutputError::InvalidVersion => (
            SafeErrorCode::HubInvalidInput,
            "The selected identification version is unavailable. Review retained issued versions.",
        ),
        LocalOutputError::InvalidTime => (
            SafeErrorCode::HubInvalidInput,
            "A captured time cannot be represented in this local document. Review the original records.",
        ),
        LocalOutputError::InvalidSnapshot => (
            SafeErrorCode::HubInvalidInput,
            "Review a current complete population snapshot before requesting local output.",
        ),
    };
    SafeError::new(code, SafeMessage::new(message))
}
