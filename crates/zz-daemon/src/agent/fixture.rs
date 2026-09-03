//! An in-process ACP agent the tests drive instead of spawning a child.
//!
//! It lives outside the host's test module because both the host's own tests
//! and the daemon's end-to-end wiring tests open panes against it.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use agent_client_protocol::{
    Agent, Client as AcpClientRole, ConnectTo, ConnectionTo,
    schema::v1::{
        AgentCapabilities, CancelRequestNotification, ContentBlock, ContentChunk,
        InitializeRequest, InitializeResponse, LoadSessionRequest, NewSessionRequest,
        NewSessionResponse, PermissionOption, PermissionOptionId, PermissionOptionKind,
        PromptRequest, PromptResponse, RequestPermissionRequest, SessionNotification,
        SessionUpdate, StopReason, TextContent, ToolCallStatus, ToolCallUpdate,
        ToolCallUpdateFields, ToolKind,
    },
};
use zz_protocol::{AgentAutoApprove, AgentProvider};

use crate::agent::{
    host::{PaneRunner, RuntimeChannels},
    runtime::run_agent_connection,
};

/// What the fixture agent does when it is prompted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Behavior {
    /// One message chunk, then the turn ends.
    Chunk,
    /// A tool call that asks permission before it settles, declaring no kind.
    AskPermission,
    /// The same, with the tool kind the risk tier judges.
    AskKindedPermission(ToolKind),
    /// A turn that never answers, so the pane stays RUNNING.
    Hang,
}

impl Behavior {
    const fn permission_kind(self) -> Option<ToolKind> {
        match self {
            Self::AskKindedPermission(kind) => Some(kind),
            _ => None,
        }
    }
}

pub(crate) fn fixture_runner(
    provider: AgentProvider,
    behavior: Behavior,
    auto_approve: AgentAutoApprove,
    load: bool,
) -> PaneRunner {
    Box::new(move |channels: RuntimeChannels| {
        Box::pin(run_agent_connection(
            provider,
            auto_approve,
            fixture_agent(behavior, load),
            channels.permission_ids,
            channels.journal,
            channels.commands,
            channels.controls,
            channels.events,
        ))
    })
}

pub(crate) fn fixture_agent(behavior: Behavior, load: bool) -> impl ConnectTo<AcpClientRole> {
    let prompts = Arc::new(AtomicUsize::new(0));
    Agent
        .builder()
        .on_receive_notification(
            async move |_: CancelRequestNotification, _| Ok(()),
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |initialize: InitializeRequest, responder, _| {
                responder.respond(
                    InitializeResponse::new(initialize.protocol_version)
                        .agent_capabilities(AgentCapabilities::new().load_session(load)),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async |_: LoadSessionRequest, responder, _| {
                responder.respond_with_error(
                    agent_client_protocol::Error::invalid_params()
                        .data("fixture session cannot be loaded"),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async |_: NewSessionRequest, responder, _| {
                responder.respond(NewSessionResponse::new("fixture-session"))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |prompt: PromptRequest,
                        responder,
                        connection: ConnectionTo<AcpClientRole>| {
                let turn = prompts.fetch_add(1, Ordering::Relaxed);
                let session_id = prompt.session_id.clone();
                connection.send_notification(SessionNotification::new(
                    session_id.clone(),
                    SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                        TextContent::new(format!("turn {turn}")),
                    ))),
                ))?;
                match behavior {
                    Behavior::Chunk => responder.respond(PromptResponse::new(StopReason::EndTurn)),
                    Behavior::Hang => Ok(()),
                    // A real adapter answers the prompt from a task of its own;
                    // awaiting the permission inline would wedge the fixture's
                    // own dispatch loop.
                    Behavior::AskPermission | Behavior::AskKindedPermission(_) => {
                        let tool = ToolCallUpdate::new(
                            "tool-1",
                            ToolCallUpdateFields::new()
                                .status(ToolCallStatus::Pending)
                                .title("run it".to_owned())
                                .kind(behavior.permission_kind()),
                        );
                        let permission = connection.send_request(RequestPermissionRequest::new(
                            session_id,
                            tool,
                            vec![
                                PermissionOption::new(
                                    PermissionOptionId::new("allow"),
                                    "Allow",
                                    PermissionOptionKind::AllowOnce,
                                ),
                                PermissionOption::new(
                                    PermissionOptionId::new("deny"),
                                    "Deny",
                                    PermissionOptionKind::RejectOnce,
                                ),
                            ],
                        ));
                        connection.spawn(async move {
                            permission.block_task().await?;
                            responder.respond(PromptResponse::new(StopReason::EndTurn))?;
                            Ok(())
                        })
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
}
