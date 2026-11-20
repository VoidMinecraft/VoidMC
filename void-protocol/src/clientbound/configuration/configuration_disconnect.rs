use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, Encode};

/// Clientbound Disconnect (Configuration) — 0x02. Disconnects a client still
/// configuring with a reason, laid out like the play state's `Disconnect`;
/// the server closes the connection after it.
#[derive(Debug, Clone, Encode, Decode)]
pub struct ConfigurationDisconnect {
    pub reason: Nbt,
}

#[cfg(test)]
mod tests {
    use ussr_nbt::owned::Tag;
    use voidmc_codec::{DecodeLimits, Encode};

    use super::*;
    use crate::clientbound::{ConfigurationPacket, Disconnect};

    fn reason() -> Nbt {
        Nbt {
            name: "".into(),
            compound: vec![(
                "translate".into(),
                Tag::String("multiplayer.disconnect.slow_login".into()),
            )]
            .into(),
        }
    }

    #[test]
    fn encodes_like_the_play_disconnect_after_packet_id_two() {
        let mut bytes = Vec::new();
        ConfigurationPacket::ConfigurationDisconnect(ConfigurationDisconnect { reason: reason() })
            .encode(&mut bytes);
        let mut body = Vec::new();
        Disconnect { reason: reason() }.encode(&mut body);

        assert_eq!(bytes[0], 0x02);
        assert_eq!(&bytes[1..], body.as_slice());
    }

    #[test]
    fn decodes_what_it_encodes() {
        let mut bytes = Vec::new();
        ConfigurationPacket::ConfigurationDisconnect(ConfigurationDisconnect { reason: reason() })
            .encode(&mut bytes);

        let decoded = DecodeLimits::default()
            .decode_exact::<ConfigurationPacket>(&bytes)
            .unwrap();
        let ConfigurationPacket::ConfigurationDisconnect(decoded) = decoded else {
            panic!("expected a ConfigurationDisconnect, got {decoded:?}");
        };
        assert_eq!(format!("{:?}", decoded.reason), format!("{:?}", reason()));
    }
}
