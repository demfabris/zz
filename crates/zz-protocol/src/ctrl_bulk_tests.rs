use crate::{Batch, CommandResponse, ProtocolMessage};

const PRE_BULK_WIRE: &[u8] = include_bytes!("fixtures/ctrl_pre_bulk_batch.bin");

#[test]
fn pre_bulk_batch_keeps_wire_raw_bytes_order_and_json_fallback() {
    let message = crate::decode_protocol_frame(PRE_BULK_WIRE).unwrap();
    assert_eq!(
        crate::encode_protocol_message(&message).unwrap(),
        PRE_BULK_WIRE
    );
    let ProtocolMessage::Batch(batch) = &message else {
        panic!("expected batch");
    };
    let messages = batch.messages().unwrap();
    assert_eq!(batch.sequence, 91);
    assert_eq!(messages.len(), 2);
    let ProtocolMessage::CommandResponse(CommandResponse::Success {
        request_id, output, ..
    }) = &messages[0]
    else {
        panic!("expected first response");
    };
    assert_eq!(*request_id, 42);
    assert_eq!(output.as_bytes(), &(0..=255).collect::<Vec<u8>>());
    let ProtocolMessage::CommandResponse(CommandResponse::Error {
        request_id, output, ..
    }) = &messages[1]
    else {
        panic!("expected second response");
    };
    assert_eq!(*request_id, 43);
    assert_eq!(output.as_bytes(), &[0xff, 0x80, b'\n']);
    let json = serde_json::to_vec(&message).unwrap();
    assert_eq!(
        serde_json::from_slice::<ProtocolMessage>(&json).unwrap(),
        message
    );
}

#[test]
fn bulk_batch_keeps_truncated_buffers_and_count_bounds() {
    let batch = Batch {
        sequence: 1,
        frames: vec![vec![1, 2, 3, 4]],
    };
    let mut bytes = postcard::to_allocvec(&batch).unwrap();
    bytes.pop();
    assert!(postcard::from_bytes::<Batch>(&bytes).is_err());
    let message = ProtocolMessage::Batch(Batch {
        sequence: 1,
        frames: vec![Vec::new(); super::MAX_BATCH_FRAMES + 1],
    });
    let bytes = postcard::to_allocvec(&message).unwrap();
    let frame = crate::framing::encode_enveloped(crate::framing::Lane::Control, &bytes).unwrap();
    assert!(crate::decode_protocol_frame(&frame).is_err());
}
