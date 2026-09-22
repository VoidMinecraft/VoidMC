use voidmc_codec::{Decode, Encode};

#[derive(Debug, Clone, Copy, Encode, Decode)]
#[repr(u8)]
pub enum GameEventType {
    NoRespawnBlockAvailable = 0,
    BeginRaining = 1,
    EndRaining = 2,
    ChangeGameMode = 3,
    WinGame = 4,
    DemoEvent = 5,
    ArrowHitPlayer = 6,
    RainLevelChange = 7,
    ThunderLevelChange = 8,
    PlayPufferfishStingSound = 9,
    PlayElderGuardianMobAppearance = 10,
    EnableRespawnScreen = 11,
    LimitedCrafting = 12,
    StartWaitingForLevelChunks = 13,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GameEvent {
    pub event: GameEventType,
    pub value: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    fn bytes(event: GameEventType, value: f32) -> Vec<u8> {
        let mut buf = Vec::new();
        PlayPacket::GameEvent(GameEvent { event, value }).encode(&mut buf);
        buf
    }

    #[test]
    fn weather_events_match_paper_ids_and_float_param() {
        assert_eq!(
            bytes(GameEventType::BeginRaining, 0.0),
            [0x26, 1, 0, 0, 0, 0]
        );
        assert_eq!(bytes(GameEventType::EndRaining, 0.0), [0x26, 2, 0, 0, 0, 0]);
        let mut expected = vec![0x26, 7];
        expected.extend_from_slice(&0.25f32.to_be_bytes());
        assert_eq!(bytes(GameEventType::RainLevelChange, 0.25), expected);
        let mut expected = vec![0x26, 8];
        expected.extend_from_slice(&1.0f32.to_be_bytes());
        assert_eq!(bytes(GameEventType::ThunderLevelChange, 1.0), expected);
    }

    #[test]
    fn other_ids_match_paper() {
        assert_eq!(bytes(GameEventType::ChangeGameMode, 1.0)[1], 3);
        assert_eq!(bytes(GameEventType::StartWaitingForLevelChunks, 0.0)[1], 13);
        let mut slice = &bytes(GameEventType::RainLevelChange, 0.5)[1..];
        let decoded = GameEvent::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert!(matches!(decoded.event, GameEventType::RainLevelChange));
        assert_eq!(decoded.value, 0.5);
    }
}
