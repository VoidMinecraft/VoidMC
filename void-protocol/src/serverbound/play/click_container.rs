use voidmc_codec::{Decode, Encode};

/// A click inside a container window (`SERVERBOUND_CONTAINER_CLICK`).
///
/// In 26.1.2 the client sends only **hashes** of what it believes each affected
/// slot / the cursor holds (`changedSlots` + `carriedItem`), not the items: the
/// server is authoritative and recomputes the result from its own inventory.
/// We therefore decode the action parameters and ignore the hashed tail
/// (`changed_hashes`), replying with the true state.
///
/// `mode` is a `ContainerInput`: 0 pickup, 1 quick-move (shift), 2 swap
/// (number key / offhand), 3 clone, 4 throw, 5 quick-craft (drag), 6 pickup-all
/// (double-click). `slot` is the window slot, or `-999` for clicking outside.
#[derive(Debug, Encode, Decode)]
pub struct ClickContainer {
    #[codec(varint32)]
    pub container_id: i32,
    #[codec(varint32)]
    pub state_id: i32,
    pub slot: i16,
    pub button: i8,
    #[codec(varint32)]
    pub mode: i32,
    /// `changedSlots` map + `carriedItem`, all hashed — ignored (authoritative).
    #[codec(remaining)]
    pub changed_hashes: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_action_and_skips_hashed_tail() {
        // container_id=0, state_id=1, slot=36, button=0, mode=0 (pickup),
        // then an empty changedSlots map (0x00) and an absent carriedItem (0x00).
        let buf = [0x00, 0x01, 0x00, 0x24, 0x00, 0x00, 0x00, 0x00];
        let mut slice = &buf[..];
        let packet = ClickContainer::decode(&mut slice).unwrap();
        assert_eq!(packet.container_id, 0);
        assert_eq!(packet.state_id, 1);
        assert_eq!(packet.slot, 36);
        assert_eq!(packet.button, 0);
        assert_eq!(packet.mode, 0);
        assert_eq!(packet.changed_hashes, vec![0x00, 0x00]);
        assert_eq!(slice.len(), 0);
    }
}
