use voidmc_codec::{Decode, Encode};

/// The player's movement input (`SERVERBOUND_PLAYER_INPUT`), sent whenever the
/// set of held movement keys changes (and used to steer vehicles).
///
/// The payload is a single bitfield byte; use the accessor methods rather than
/// reading [`flags`](Self::flags) directly.
#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct PlayerInput {
    pub flags: u8,
}

impl PlayerInput {
    pub const FORWARD: u8 = 0x01;
    pub const BACKWARD: u8 = 0x02;
    pub const LEFT: u8 = 0x04;
    pub const RIGHT: u8 = 0x08;
    pub const JUMP: u8 = 0x10;
    pub const SNEAK: u8 = 0x20;
    pub const SPRINT: u8 = 0x40;

    pub fn forward(self) -> bool {
        self.flags & Self::FORWARD != 0
    }
    pub fn backward(self) -> bool {
        self.flags & Self::BACKWARD != 0
    }
    pub fn left(self) -> bool {
        self.flags & Self::LEFT != 0
    }
    pub fn right(self) -> bool {
        self.flags & Self::RIGHT != 0
    }
    pub fn jump(self) -> bool {
        self.flags & Self::JUMP != 0
    }
    pub fn sneak(self) -> bool {
        self.flags & Self::SNEAK != 0
    }
    pub fn sprint(self) -> bool {
        self.flags & Self::SPRINT != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_flag_bits() {
        // forward + jump + sprint = 0x01 | 0x10 | 0x40 = 0x51
        let buf = [0x51u8];
        let mut slice = &buf[..];
        let input = PlayerInput::decode(&mut slice).unwrap();
        assert!(input.forward() && input.jump() && input.sprint());
        assert!(!input.backward() && !input.left() && !input.right() && !input.sneak());
        assert_eq!(slice.len(), 0);
    }
}
