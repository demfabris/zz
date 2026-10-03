use std::fmt::Debug;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::{
    AgentImage, ClientFileOperation, ClientFileResponse, ClientPath,
    MAX_CLIENT_WORKING_DIRECTORY_BYTES, RawText,
};
use crate::{Batch, EventPayload, PaneId, ProtocolMessage};

const LENGTHS: &[usize] = &[0, 1, 127, 128, 300, 16_384, 70_000];

fn sample(len: usize) -> Vec<u8> {
    (0..len)
        .map(|index| (index.wrapping_mul(131).wrapping_add(7) % 256) as u8)
        .collect()
}

fn assert_same_wire<New, Old>(new: &New, old: &Old)
where
    New: Serialize + DeserializeOwned + PartialEq + Debug,
    Old: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let new_wire = postcard::to_allocvec(new).unwrap();
    let old_wire = postcard::to_allocvec(old).unwrap();
    assert_eq!(new_wire, old_wire);
    assert_eq!(postcard::from_bytes::<New>(&old_wire).unwrap(), *new);
    assert_eq!(postcard::from_bytes::<Old>(&new_wire).unwrap(), *old);
    let new_json = serde_json::to_vec(new).unwrap();
    let old_json = serde_json::to_vec(old).unwrap();
    assert_eq!(new_json, old_json);
    assert_eq!(serde_json::from_slice::<New>(&old_json).unwrap(), *new);
    assert_eq!(serde_json::from_slice::<Old>(&new_json).unwrap(), *old);
}

fn assert_same_variant_wire<New, Old>(new: &New, old: &Old)
where
    New: Serialize + DeserializeOwned + PartialEq + Debug,
    Old: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let new_wire = postcard::to_allocvec(new).unwrap();
    let old_body = postcard::to_allocvec(old).unwrap();
    let (_, new_body) = postcard::take_from_bytes::<u32>(&new_wire).unwrap();
    assert_eq!(new_body, old_body);
    let tag = &new_wire[..new_wire.len() - new_body.len()];
    assert_eq!(
        postcard::from_bytes::<New>(&[tag, &old_body].concat()).unwrap(),
        *new
    );
    assert_eq!(postcard::from_bytes::<Old>(new_body).unwrap(), *old);
    let serde_json::Value::Object(new_json) = serde_json::to_value(new).unwrap() else {
        panic!("expected an externally tagged variant");
    };
    let (name, new_inner) = new_json.into_iter().next().unwrap();
    let old_json = serde_json::to_value(old).unwrap();
    assert_eq!(new_inner, old_json);
    assert_eq!(serde_json::from_value::<Old>(new_inner).unwrap(), *old);
    let mut tagged = serde_json::Map::new();
    tagged.insert(name, old_json);
    assert_eq!(
        serde_json::from_value::<New>(serde_json::Value::Object(tagged)).unwrap(),
        *new
    );
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OldPaneOutput {
    pane: PaneId,
    bytes: Vec<u8>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OldPaneOutputAged {
    pane: PaneId,
    age_ms: u64,
    bytes: Vec<u8>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OldKittyImageChunk {
    pane: PaneId,
    image_id: u32,
    generation: u64,
    bytes: Vec<u8>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OldAgentUpdates {
    pane: PaneId,
    first_seq: u64,
    items: Vec<Vec<u8>>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OldPasteUploadChunk {
    upload_id: u64,
    bytes: Vec<u8>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OldPastedImageChunk {
    pane: PaneId,
    number: u32,
    bytes: Vec<u8>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OldControlStdin {
    bytes: Vec<u8>,
    submitted: bool,
    closed: bool,
    error: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OldControlWrite {
    bytes: Vec<u8>,
    idle: Option<(u64, u64)>,
    close: bool,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OldClientFileWrite {
    append: bool,
    data: Vec<u8>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OldClientFileResponse {
    request_id: u64,
    data: Vec<u8>,
    error: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OldAgentImage {
    format: String,
    data: Vec<u8>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OldClientPath(Vec<u8>);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OldBatch {
    sequence: u64,
    frames: Vec<Vec<u8>>,
}

#[test]
fn event_byte_fields_keep_the_sequence_wire() {
    for &len in LENGTHS {
        let bytes = sample(len);
        let pane = PaneId(len as u64);
        assert_same_variant_wire(
            &EventPayload::PaneOutput {
                pane,
                bytes: bytes.clone(),
            },
            &OldPaneOutput {
                pane,
                bytes: bytes.clone(),
            },
        );
        assert_same_variant_wire(
            &EventPayload::PaneOutputAged {
                pane,
                age_ms: 300,
                bytes: bytes.clone(),
            },
            &OldPaneOutputAged {
                pane,
                age_ms: 300,
                bytes: bytes.clone(),
            },
        );
        assert_same_variant_wire(
            &EventPayload::KittyImageChunk {
                pane,
                image_id: 9,
                generation: 1 << 40,
                bytes: bytes.clone(),
            },
            &OldKittyImageChunk {
                pane,
                image_id: 9,
                generation: 1 << 40,
                bytes: bytes.clone(),
            },
        );
        let items = vec![bytes.clone(), Vec::new(), sample(len / 2 + 1)];
        assert_same_variant_wire(
            &EventPayload::AgentUpdates {
                pane,
                first_seq: 200,
                items: items.clone(),
            },
            &OldAgentUpdates {
                pane,
                first_seq: 200,
                items,
            },
        );
    }
}

#[test]
fn client_byte_fields_keep_the_sequence_wire() {
    for &len in LENGTHS {
        let bytes = sample(len);
        let pane = PaneId(len as u64);
        assert_same_variant_wire(
            &ProtocolMessage::PasteUploadChunk {
                upload_id: 77,
                bytes: bytes.clone(),
            },
            &OldPasteUploadChunk {
                upload_id: 77,
                bytes: bytes.clone(),
            },
        );
        assert_same_variant_wire(
            &ProtocolMessage::PastedImageChunk {
                pane,
                number: 3,
                bytes: bytes.clone(),
            },
            &OldPastedImageChunk {
                pane,
                number: 3,
                bytes: bytes.clone(),
            },
        );
        assert_same_variant_wire(
            &ProtocolMessage::ControlStdin {
                bytes: bytes.clone(),
                submitted: true,
                closed: false,
                error: Some("gone".to_owned()),
            },
            &OldControlStdin {
                bytes: bytes.clone(),
                submitted: true,
                closed: false,
                error: Some("gone".to_owned()),
            },
        );
        assert_same_variant_wire(
            &ProtocolMessage::ControlWrite {
                bytes: bytes.clone(),
                idle: Some((5, 1 << 33)),
                close: true,
            },
            &OldControlWrite {
                bytes: bytes.clone(),
                idle: Some((5, 1 << 33)),
                close: true,
            },
        );
        assert_same_variant_wire(
            &ClientFileOperation::Write {
                append: true,
                data: bytes.clone(),
            },
            &OldClientFileWrite {
                append: true,
                data: bytes.clone(),
            },
        );
    }
}

#[test]
fn byte_structs_keep_the_sequence_wire() {
    for &len in LENGTHS {
        let bytes = sample(len);
        assert_same_wire(
            &ClientFileResponse {
                request_id: 12,
                data: bytes.clone(),
                error: None,
            },
            &OldClientFileResponse {
                request_id: 12,
                data: bytes.clone(),
                error: None,
            },
        );
        assert_same_wire(
            &AgentImage {
                format: "image/png".to_owned(),
                data: bytes.clone(),
            },
            &OldAgentImage {
                format: "image/png".to_owned(),
                data: bytes.clone(),
            },
        );
        let path = sample(len.min(MAX_CLIENT_WORKING_DIRECTORY_BYTES));
        assert_same_wire(&ClientPath(path.clone()), &OldClientPath(path));
        assert_same_wire(&RawText::from_bytes(bytes.clone()), &bytes);
        let frames = vec![bytes.clone(), Vec::new(), sample(len / 3 + 2)];
        assert_same_wire(
            &Batch {
                sequence: 1 << 20,
                frames: frames.clone(),
            },
            &OldBatch {
                sequence: 1 << 20,
                frames,
            },
        );
    }
}
