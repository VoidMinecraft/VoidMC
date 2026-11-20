use serde::{Deserialize, Serialize};
use voidmc_codec::{Decode, Encode};

/// A text component in its JSON form, which the login state uses where
/// later states use NBT. Only the fields the server sends are modelled.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct JsonTextComponent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A key the client looks up in its language file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translate: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// Clientbound Login Disconnect (Login) — 0x00. Refuses a login with a
/// reason; the server closes the connection after it.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct LoginDisconnect {
    #[codec(json)]
    pub reason: JsonTextComponent,
}

#[cfg(test)]
mod tests {
    use voidmc_codec::{DecodeLimits, Encode};

    use super::*;
    use crate::clientbound::LoginPacket;

    fn encoded(packet: &LoginPacket) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet.encode(&mut bytes);
        bytes
    }

    #[test]
    fn encodes_the_reason_as_a_json_string_after_packet_id_zero() {
        let packet = LoginPacket::LoginDisconnect(LoginDisconnect {
            reason: JsonTextComponent {
                translate: Some("multiplayer.disconnect.server_full".to_string()),
                ..Default::default()
            },
        });
        let json = r#"{"translate":"multiplayer.disconnect.server_full"}"#;

        let mut expected = vec![0x00, json.len() as u8];
        expected.extend_from_slice(json.as_bytes());
        assert_eq!(encoded(&packet), expected);
    }

    #[test]
    fn decodes_what_it_encodes() {
        let packet = LoginPacket::LoginDisconnect(LoginDisconnect {
            reason: JsonTextComponent {
                text: Some("Ünïcode \"quoted\" \\ end".to_string()),
                color: Some("red".to_string()),
                ..Default::default()
            },
        });

        let decoded = DecodeLimits::default()
            .decode_exact::<LoginPacket>(&encoded(&packet))
            .unwrap();
        let LoginPacket::LoginDisconnect(decoded) = decoded else {
            panic!("expected a LoginDisconnect, got {decoded:?}");
        };
        let LoginPacket::LoginDisconnect(packet) = packet else {
            unreachable!()
        };
        assert_eq!(decoded, packet);
    }
}
