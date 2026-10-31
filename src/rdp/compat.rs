//! Fixes for what servers send that IronRDP doesn't yet take as it comes.

use ironrdp_pdu::mcs;

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
    fn leaves_other_pdus_alone() {
        for pdu_type in [REDIRECTION, 0x0017] {
            let mut bytes = frame(pdu_type);
            assert!(!normalize_redirection(&mut bytes));
            assert_eq!(bytes, frame(pdu_type));
        }
    }
}
