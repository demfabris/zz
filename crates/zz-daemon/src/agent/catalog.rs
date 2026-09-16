use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};

use parking_lot::Mutex;
use serde_json::Value;
use zz_protocol::{AgentAutoApprove, AgentProvider};

use super::{
    host::{PaneRunner, RuntimeChannels},
    runtime::{AgentSpawnConfig, RuntimeCommand, RuntimeControl, run_agent_runtime},
    stream::AgentStreamPayload,
};

pub(crate) async fn load(
    mut config: AgentSpawnConfig,
    provider: AgentProvider,
    cwd: PathBuf,
) -> Result<Value, String> {
    config.auto_approve = AgentAutoApprove::Off;
    let runner: PaneRunner = Box::new(move |channels| {
        Box::pin(run_agent_runtime(
            config,
            provider,
            channels.auto_approve,
            channels.permission_ids,
            None,
            channels.commands,
            channels.controls,
            channels.events,
        ))
    });
    load_with_runner(cwd, runner).await
}

async fn load_with_runner(cwd: PathBuf, runner: PaneRunner) -> Result<Value, String> {
    let (commands, command_rx) = async_channel::bounded(2);
    let (controls, control_rx) = async_channel::bounded(4);
    let (events, event_rx) = async_channel::bounded(32);
    commands
        .try_send(RuntimeCommand::Open {
            cwd,
            resume_session: None,
        })
        .map_err(|error| error.to_string())?;
    let closer = events.clone();
    let runtime = async move {
        let result = runner(RuntimeChannels {
            auto_approve: Arc::new(Mutex::new(AgentAutoApprove::Off)),
            permission_ids: Arc::new(AtomicU64::new(1)),
            journal: None,
            commands: command_rx,
            controls: control_rx,
            events,
        })
        .await;
        closer.close();
        result
    };
    let collect = async move {
        let mut result = None;
        while let Ok(event) = event_rx.recv().await {
            match event {
                AgentStreamPayload::SessionReady { config_options, .. } => {
                    result = Some(Ok(config_options.unwrap_or_else(|| serde_json::json!([]))));
                    let _ = commands.send(RuntimeCommand::Shutdown).await;
                }
                AgentStreamPayload::PaneFailed { message }
                | AgentStreamPayload::AuthenticationFailed { message } => {
                    result = Some(Err(message));
                    let _ = commands.send(RuntimeCommand::Shutdown).await;
                }
                AgentStreamPayload::PermissionRequested { request_id, .. } => {
                    let _ = controls
                        .send(RuntimeControl::RespondPermission {
                            request_id,
                            option_id: None,
                        })
                        .await;
                }
                _ => {}
            }
        }
        result
    };
    futures_lite::future::race(
        async {
            let (runtime, result) = futures_lite::future::zip(runtime, collect).await;
            match result {
                Some(result) => result,
                None => Err(runtime.err().unwrap_or_else(|| {
                    "The agent closed before returning its model catalog.".into()
                })),
            }
        },
        async {
            smol::Timer::after(Duration::from_secs(35)).await;
            Err("Timed out loading the agent model catalog.".into())
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_opens_an_isolated_session_and_shuts_it_down_without_prompting() {
        let runner: PaneRunner = Box::new(|channels| {
            Box::pin(async move {
                assert!(channels.journal.is_none());
                assert!(
                    matches!(channels.commands.recv().await.unwrap(), RuntimeCommand::Open { cwd, resume_session: None } if cwd == std::path::Path::new("/workspace"))
                );
                channels
                    .events
                    .send(AgentStreamPayload::SessionReady {
                        session_id: "temporary".into(),
                        modes: None,
                        config_options: Some(
                            serde_json::json!([{ "id": "model", "currentValue": "fast" }]),
                        ),
                    })
                    .await
                    .unwrap();
                assert!(matches!(
                    channels.commands.recv().await.unwrap(),
                    RuntimeCommand::Shutdown
                ));
                Ok(())
            })
        });
        let catalog =
            futures_lite::future::block_on(load_with_runner("/workspace".into(), runner)).unwrap();
        assert_eq!(catalog[0]["currentValue"], "fast");
    }

    #[test]
    fn catalog_reads_the_advertised_options_over_acp() {
        use agent_client_protocol::{
            Agent,
            schema::v1::{
                InitializeRequest, InitializeResponse, NewSessionRequest, NewSessionResponse,
            },
        };
        let agent = Agent.builder()
            .on_receive_request(async |initialize: InitializeRequest, responder, _| {
                responder.respond(InitializeResponse::new(initialize.protocol_version))
            }, agent_client_protocol::on_receive_request!())
            .on_receive_request(async |request: NewSessionRequest, responder, _| {
                assert_eq!(request.cwd, PathBuf::from("/workspace"));
                let response: NewSessionResponse = serde_json::from_value(serde_json::json!({
                    "sessionId": "catalog-only",
                    "configOptions": [{"id": "model", "name": "Model", "category": "model", "type": "select", "currentValue": "fast", "options": [{"value": "fast", "name": "Fast"}]}]
                })).unwrap();
                responder.respond(response)
            }, agent_client_protocol::on_receive_request!());
        let runner: PaneRunner = Box::new(move |channels| {
            Box::pin(super::super::runtime::run_agent_connection(
                AgentProvider::ClaudeCode,
                channels.auto_approve,
                agent,
                channels.permission_ids,
                None,
                channels.commands,
                channels.controls,
                channels.events,
            ))
        });
        let result =
            futures_lite::future::block_on(load_with_runner("/workspace".into(), runner)).unwrap();
        assert_eq!(result[0]["currentValue"], "fast");
        assert_eq!(result[0]["options"][0]["name"], "Fast");
    }

    #[test]
    fn catalog_reports_startup_failure() {
        let runner: PaneRunner = Box::new(|_| Box::pin(async { Err("not authenticated".into()) }));
        assert_eq!(
            futures_lite::future::block_on(load_with_runner("/workspace".into(), runner)),
            Err("not authenticated".into())
        );
    }
}
