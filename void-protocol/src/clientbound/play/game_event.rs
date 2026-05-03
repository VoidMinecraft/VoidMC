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
