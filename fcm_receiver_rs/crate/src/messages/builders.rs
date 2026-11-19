use crate::consts::{
    KMCS_VERSION, K_HEARTBEAT_ACK_TAG, K_HEARTBEAT_PING_TAG, K_IQ_STANZA_TAG, K_LOGIN_REQUEST_TAG,
    K_SELECTIVE_ACK_EXTENSION, K_STREAM_ACK_EXTENSION,
};
use crate::error::Result;
use crate::proto::put_uvarint;

use super::types::*;
use super::{
    marshal_heartbeat_ack, marshal_heartbeat_ping, marshal_iq_stanza, marshal_login_request,
    marshal_selective_ack, unmarshal_close, unmarshal_data_message_stanza, unmarshal_heartbeat_ack,
    unmarshal_heartbeat_ping, unmarshal_iq_stanza, unmarshal_login_request,
    unmarshal_login_response, unmarshal_stream_error_stanza,
};

pub fn create_login_request_raw(
    android_id: u64,
    security_token: u64,
    persistent_ids: &[String],
) -> Result<Vec<u8>> {
    let chrome_version = "chrome-63.0.3234.0";
    let domain = "mcs.android.com";

    let android_id_formatted = android_id.to_string();
    let android_id_hex = format!("android-{android_id:x}");
    let security_token_formatted = security_token.to_string();

    let request = ManualLoginRequest {
        id: chrome_version.to_string(),
        domain: domain.to_string(),
        user: android_id_formatted.clone(),
        resource: android_id_formatted.clone(),
        auth_token: security_token_formatted,
        device_id: android_id_hex,
        last_rmq_id: None,
        setting: vec![ManualSetting {
            name: "new_vc".to_string(),
            value: "1".to_string(),
        }],
        // Every message received and not yet confirmed: the server takes
        // these as acknowledged and doesn't deliver them again.
        received_persistent_id: persistent_ids.to_vec(),
        adaptive_heartbeat: Some(false),
        heartbeat_stat: None,
        use_rmq2: Some(true),
        account_id: None,
        auth_service: Some(ManualLoginRequestAuthService::AndroidId),
        network_type: Some(1),
        status: None,
        client_event: Vec::new(),
    };

    let payload = marshal_login_request(&request)?;

    let mut packet = Vec::with_capacity(2 + 5 + payload.len());
    packet.push(KMCS_VERSION);
    packet.push(K_LOGIN_REQUEST_TAG);
    packet.extend_from_slice(&put_uvarint(payload.len() as u64));
    packet.extend_from_slice(&payload);
    Ok(packet)
}

/// A stanza sent after the login request: its tag, its size and its bytes
/// (only the login request starts with the protocol version).
pub fn frame(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(1 + 5 + payload.len());
    packet.push(tag);
    packet.extend_from_slice(&put_uvarint(payload.len() as u64));
    packet.extend_from_slice(payload);
    packet
}

/// A heartbeat ping, acknowledging the stanzas received up to
/// `last_stream_id_received`.
pub fn create_heartbeat_ping_raw(last_stream_id_received: i32) -> Result<Vec<u8>> {
    let ping = ManualHeartbeatPing {
        last_stream_id_received: Some(last_stream_id_received),
        ..Default::default()
    };
    Ok(frame(K_HEARTBEAT_PING_TAG, &marshal_heartbeat_ping(&ping)?))
}

/// The answer to the server's heartbeat ping, acknowledging the stanzas
/// received up to `last_stream_id_received`.
pub fn create_heartbeat_ack_raw(last_stream_id_received: i32) -> Result<Vec<u8>> {
    let ack = ManualHeartbeatAck {
        last_stream_id_received: Some(last_stream_id_received),
        ..Default::default()
    };
    Ok(frame(K_HEARTBEAT_ACK_TAG, &marshal_heartbeat_ack(&ack)?))
}

fn ack_iq(extension: i32, data: Vec<u8>, last_stream_id_received: i32) -> Result<Vec<u8>> {
    let iq = ManualIqStanza {
        stanza_type: Some(ManualIqStanzaType::Set),
        // Required, and empty for acks (as Chromium sends them).
        id: Some(String::new()),
        extension: Some(ManualExtension {
            id: Some(extension),
            data,
        }),
        last_stream_id_received: Some(last_stream_id_received),
        ..Default::default()
    };
    Ok(frame(K_IQ_STANZA_TAG, &marshal_iq_stanza(&iq)?))
}

/// A stream ack: everything received up to `last_stream_id_received` is
/// acknowledged, so the server doesn't deliver it again.
pub fn create_stream_ack_raw(last_stream_id_received: i32) -> Result<Vec<u8>> {
    ack_iq(K_STREAM_ACK_EXTENSION, Vec::new(), last_stream_id_received)
}

/// A selective ack: the messages with these persistent ids are
/// acknowledged, whenever they were received.
pub fn create_selective_ack_raw(ids: &[String], last_stream_id_received: i32) -> Result<Vec<u8>> {
    let ack = ManualSelectiveAck { id: ids.to_vec() };
    ack_iq(
        K_SELECTIVE_ACK_EXTENSION,
        marshal_selective_ack(&ack)?,
        last_stream_id_received,
    )
}

pub fn create_check_in_request(
    android_id: i64,
    security_token: Option<u64>,
) -> ManualAndroidCheckinRequest {
    let mut request = ManualAndroidCheckinRequest::default();
    request.id = Some(android_id);
    request.version = Some(3);
    request.security_token = security_token;

    let chrome_build = ManualChromeBuildProto {
        platform: Some(ManualChromeBuildPlatform::Linux),
        chrome_version: Some("63.0.3234.0".to_string()),
        channel: Some(ManualChromeBuildChannel::Stable),
    };

    request.checkin = Some(ManualAndroidCheckinProto {
        last_checkin_msec: None,
        cell_operator: None,
        sim_operator: None,
        roaming: None,
        user_number: None,
        device_type: Some(ManualDeviceType::ChromeBrowser),
        chrome_build: Some(chrome_build),
    });

    request
}

pub fn decode_heartbeat_ping(data: &[u8]) -> Result<ManualHeartbeatPing> {
    unmarshal_heartbeat_ping(data)
}

pub fn decode_heartbeat_ack(data: &[u8]) -> Result<ManualHeartbeatAck> {
    unmarshal_heartbeat_ack(data)
}

pub fn decode_login_request(data: &[u8]) -> Result<ManualLoginRequest> {
    unmarshal_login_request(data)
}

pub fn decode_login_response(data: &[u8]) -> Result<ManualLoginResponse> {
    unmarshal_login_response(data)
}

pub fn decode_close(data: &[u8]) -> Result<ManualClose> {
    unmarshal_close(data)
}

pub fn decode_iq_stanza(data: &[u8]) -> Result<ManualIqStanza> {
    unmarshal_iq_stanza(data)
}

pub fn decode_data_message_stanza(data: &[u8]) -> Result<ManualDataMessageStanza> {
    unmarshal_data_message_stanza(data)
}

pub fn decode_stream_error_stanza(data: &[u8]) -> Result<ManualStreamErrorStanza> {
    unmarshal_stream_error_stanza(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::{K_HEARTBEAT_ACK_TAG, K_HEARTBEAT_PING_TAG, K_IQ_STANZA_TAG};
    use crate::messages::{unmarshal_extension, unmarshal_login_request};
    use crate::proto::ProtoDecoder;

    /// A framed stanza's tag and payload.
    fn unframe(packet: &[u8]) -> (u8, Vec<u8>) {
        let mut size = 0usize;
        let mut shift = 0;
        let mut i = 1;
        loop {
            let byte = packet[i];
            size |= ((byte & 0x7f) as usize) << shift;
            i += 1;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }
        assert_eq!(packet.len(), i + size, "the size covers the payload");
        (packet[0], packet[i..].to_vec())
    }

    #[test]
    fn login_names_every_received_persistent_id() {
        let ids: Vec<String> = (0..5).map(|i| format!("0:{i}")).collect();
        let packet = create_login_request_raw(1, 2, &ids).unwrap();
        assert_eq!(packet[0], KMCS_VERSION);
        let (tag, payload) = unframe(&packet[1..]);
        assert_eq!(tag, K_LOGIN_REQUEST_TAG);
        let request = unmarshal_login_request(&payload).unwrap();
        assert_eq!(request.received_persistent_id, ids);
    }

    #[test]
    fn selective_ack_names_its_ids() {
        let ids = vec!["0:1".to_string(), "0:2".to_string()];
        let (tag, payload) = unframe(&create_selective_ack_raw(&ids, 7).unwrap());
        assert_eq!(tag, K_IQ_STANZA_TAG);
        let iq = decode_iq_stanza(&payload).unwrap();
        assert_eq!(iq.stanza_type, Some(ManualIqStanzaType::Set));
        assert_eq!(iq.id.as_deref(), Some(""));
        assert_eq!(iq.last_stream_id_received, Some(7));
        let extension = iq.extension.unwrap();
        assert_eq!(extension.id, Some(K_SELECTIVE_ACK_EXTENSION));
        let mut decoder = ProtoDecoder::new(&extension.data);
        let mut acked = Vec::new();
        while decoder.remaining() > 0 {
            let (field, _) = decoder.decode_field_number().unwrap();
            assert_eq!(field, 1);
            acked.push(decoder.decode_string().unwrap());
        }
        assert_eq!(acked, ids);
    }

    #[test]
    fn stream_ack_carries_its_required_empty_data() {
        let (tag, payload) = unframe(&create_stream_ack_raw(3).unwrap());
        assert_eq!(tag, K_IQ_STANZA_TAG);
        let iq = decode_iq_stanza(&payload).unwrap();
        assert_eq!(iq.last_stream_id_received, Some(3));
        let extension = iq.extension.unwrap();
        assert_eq!(extension.id, Some(K_STREAM_ACK_EXTENSION));
        assert!(extension.data.is_empty());
        // The extension's `data` (field 2) is there, though empty: mcs.proto
        // requires it. Re-encoded as the iq carries it: id 13, then data.
        let raw = crate::messages::marshal_extension(&extension).unwrap();
        assert_eq!(raw, vec![0x08, 13, 0x12, 0x00]);
        assert_eq!(unmarshal_extension(&raw).unwrap().id, Some(13));
    }

    #[test]
    fn heartbeats_acknowledge_the_stream() {
        let (tag, payload) = unframe(&create_heartbeat_ack_raw(9).unwrap());
        assert_eq!(tag, K_HEARTBEAT_ACK_TAG);
        assert_eq!(
            decode_heartbeat_ack(&payload).unwrap().last_stream_id_received,
            Some(9)
        );
        let (tag, payload) = unframe(&create_heartbeat_ping_raw(4).unwrap());
        assert_eq!(tag, K_HEARTBEAT_PING_TAG);
        assert_eq!(
            decode_heartbeat_ping(&payload).unwrap().last_stream_id_received,
            Some(4)
        );
    }
}
