use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

fn emit(value: Value) {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, &value).unwrap();
    output.write_all(b"\n").unwrap();
    output.flush().unwrap();
}

fn respond(id: Value, result: Value) {
    emit(json!({"jsonrpc":"2.0","id":id,"result":result}));
}
fn update(session: &str, update: Value) {
    emit(
        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,"update":update}}),
    );
}
fn options(model: &str) -> Value {
    json!([{"id":"model","name":"Model","category":"model","type":"select","currentValue":model,
        "options":[{"value":"small","name":"Small"},{"value":"large","name":"Large"}]}])
}

pub fn run() {
    let mut session_number = 0;
    let prefix = format!("native-fixture-{}", std::process::id());
    let saved_session = format!("{prefix}-saved");
    let mut session = String::new();
    let mut pending = None;
    let mut model = "small".to_owned();
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else {
            break;
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = value["id"].clone();
        let params = &value["params"];
        match value["method"].as_str() {
            Some("initialize") => respond(
                id,
                json!({"protocolVersion":params["protocolVersion"],
                "agentInfo":{"name":"native-fixture","title":"Native fixture","version":"1"},
                "agentCapabilities":{"loadSession":true,"promptCapabilities":{"image":true},"sessionCapabilities":{"list":{}}},
                "authMethods":[]}),
            ),
            Some("session/new") => {
                model = "small".to_owned();
                session_number += 1;
                session = format!("{prefix}-session-{session_number}");
                respond(
                    id,
                    json!({"sessionId":session,"configOptions":options(&model)}),
                );
                update(
                    &session,
                    json!({"sessionUpdate":"available_commands_update","availableCommands":[{"name":"review","description":"Review a path","input":{"hint":"path"}}]}),
                );
            }
            Some("session/prompt") => {
                let text = params["prompt"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|block| block["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                for content in params["prompt"].as_array().unwrap() {
                    update(
                        &session,
                        json!({"sessionUpdate":"user_message_chunk","content":content}),
                    );
                }
                update(
                    &session,
                    json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":format!("Reply: {text}")}}),
                );
                if text == "permission" {
                    pending = Some(id);
                    emit(
                        json!({"jsonrpc":"2.0","id":"permission-1","method":"session/request_permission","params":{
                        "sessionId":session,"toolCall":{"toolCallId":"tool-1","title":"Read fixture file","kind":"read","status":"pending"},
                        "options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"},{"optionId":"deny","name":"Deny","kind":"reject_once"}]}}),
                    );
                } else if text == "hang" {
                    pending = Some(id);
                } else {
                    respond(id, json!({"stopReason":"end_turn"}));
                }
            }
            Some("session/cancel") => {
                if let Some(id) = pending.take() {
                    respond(id, json!({"stopReason":"cancelled"}));
                }
            }
            Some("session/set_config_option") => {
                model = params["value"].as_str().unwrap().to_owned();
                respond(id, json!({"configOptions":options(&model)}));
            }
            Some("session/list") => respond(
                id,
                json!({"sessions":[{"sessionId":saved_session,"cwd":"/tmp","title":"Saved fixture"}]}),
            ),
            Some("session/load") => {
                session = params["sessionId"].as_str().unwrap().to_owned();
                update(
                    &session,
                    json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Restored conversation"}}),
                );
                respond(id, json!({"configOptions":options(&model)}));
            }
            Some(method) => {
                if !id.is_null() {
                    emit(
                        json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":format!("unsupported {method}")}}),
                    );
                }
            }
            None if id == "permission-1" => {
                if let Some(id) = pending.take() {
                    respond(id, json!({"stopReason":"end_turn"}));
                }
            }
            None => {}
        }
    }
}
