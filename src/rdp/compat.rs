//! Fixes for what servers send that IronRDP doesn't yet take as it comes.

use ironrdp_pdu::mcs;
use ironrdp_pdu::rdp::headers::{self, ShareDataPdu};
use ironrdp_pdu::rdp::server_error_info::{ErrorInfo, ServerSetErrorInfoPdu};

/// The Enhanced Security Server Redirection PDU's `pduType`, as the spec
/// has it: type 0xA with version 0 ([MS-RDPBCGR] 2.2.13.3.1).
const REDIRECTION: u16 = 0x000A;
/// The same with version 1, as FreeRDP writes every Share Control Header.
const REDIRECTION_V1: u16 = 0x001A;

/// GNOME Remote Desktop hands a Remote Login session over with a Server
/// Redirection PDU it builds with FreeRDP, which sets the header's version
/// to 1; IronRDP takes only 0 there and fails the session. Rewrite the
/// version in place so the PDU decodes. Returns whether it did.
pub(super) fn normalize_redirection(frame: &mut [u8]) -> bool {
    let Ok(indication) = mcs::decode_send_data_indication(frame) else {
        return false;
    };
    let data = indication.user_data;
    // pduType follows totalLength in the Share Control Header.
    let Some(kind) = data.get(2..4) else {
        return false;
    };
    if u16::from_le_bytes([kind[0], kind[1]]) != REDIRECTION_V1 {
        return false;
    }
    let offset = data.as_ptr() as usize - frame.as_ptr() as usize + 2;
    frame[offset..offset + 2].copy_from_slice(&REDIRECTION.to_le_bytes());
    true
}

/// The reason in a Set Error Info PDU, which IronRDP turns into English
/// before it reaches here.
pub(super) fn error_info(frame: &[u8]) -> Option<ErrorInfo> {
    let indication = mcs::decode_send_data_indication(frame).ok()?;
    match headers::decode_share_data(indication).ok()?.pdu {
        ShareDataPdu::ServerSetErrorInfo(ServerSetErrorInfoPdu(info)) => Some(info),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use ironrdp_pdu::mcs::{McsMessage, SendDataIndication};
    use ironrdp_pdu::x224::X224;

    use super::*;

    fn frame(pdu_type: u16) -> Vec<u8> {
        let mut user_data = vec![0x20, 0x00];
        user_data.extend_from_slice(&pdu_type.to_le_bytes());
        user_data.extend_from_slice(&[0xEA, 0x03, 0x00, 0x00]);
        ironrdp_core::encode_vec(&X224(McsMessage::SendDataIndication(SendDataIndication {
            initiator_id: 1002,
            channel_id: 1003,
            user_data: Cow::Owned(user_data),
        })))
        .expect("encodes")
    }

    #[test]
    fn rewrites_freerdps_redirection_header() {
        let mut bytes = frame(REDIRECTION_V1);
        assert!(normalize_redirection(&mut bytes));
        assert_eq!(bytes, frame(REDIRECTION));
    }

    #[test]
    fn reads_the_reason_a_server_gives() {
        use ironrdp_pdu::rdp::client_info::CompressionType;
        use ironrdp_pdu::rdp::headers::{
            CompressionFlags, ShareControlHeader, ShareControlPdu, ShareDataHeader, StreamPriority,
        };
        use ironrdp_pdu::rdp::server_error_info::ProtocolIndependentCode;

        let taken_over = ErrorInfo::ProtocolIndependentCode(
            ProtocolIndependentCode::DisconnectedByOtherconnection,
        );
        let header = ShareControlHeader {
            share_control_pdu: ShareControlPdu::Data(ShareDataHeader {
                share_data_pdu: ShareDataPdu::ServerSetErrorInfo(ServerSetErrorInfoPdu(taken_over)),
                stream_priority: StreamPriority::Low,
                compression_flags: CompressionFlags::empty(),
                compression_type: CompressionType::K8,
            }),
            pdu_source: 1002,
            share_id: 0x0001_03ea,
        };
        let bytes =
            ironrdp_core::encode_vec(&X224(McsMessage::SendDataIndication(SendDataIndication {
                initiator_id: 1002,
                channel_id: 1003,
                user_data: Cow::Owned(ironrdp_core::encode_vec(&header).expect("encodes")),
            })))
            .expect("encodes");
        assert!(matches!(
            error_info(&bytes),
            Some(ErrorInfo::ProtocolIndependentCode(
                ProtocolIndependentCode::DisconnectedByOtherconnection
            ))
        ));
        assert!(error_info(&frame(0x0017)).is_none());
    }

    #[test]
    fn leaves_other_pdus_alone() {
        for pdu_type in [REDIRECTION, 0x0017] {
            let mut bytes = frame(pdu_type);
            assert!(!normalize_redirection(&mut bytes));
            assert_eq!(bytes, frame(pdu_type));
        }
    }
}
