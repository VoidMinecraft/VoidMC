use voidmc_codec::{Decode, Encode};

/// An empty `name` clears the slot.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct SetDisplayObjective {
    pub slot: DisplaySlot,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum DisplaySlot {
    List = 0,
    #[default]
    Sidebar = 1,
    BelowName = 2,
    SidebarTeamBlack = 3,
    SidebarTeamDarkBlue = 4,
    SidebarTeamDarkGreen = 5,
    SidebarTeamDarkAqua = 6,
    SidebarTeamDarkRed = 7,
    SidebarTeamDarkPurple = 8,
    SidebarTeamGold = 9,
    SidebarTeamGray = 10,
    SidebarTeamDarkGray = 11,
    SidebarTeamBlue = 12,
    SidebarTeamGreen = 13,
    SidebarTeamAqua = 14,
    SidebarTeamRed = 15,
    SidebarTeamLightPurple = 16,
    SidebarTeamYellow = 17,
    SidebarTeamWhite = 18,
}

#[cfg(test)]
mod tests {
    use super::super::number_format::string;
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn slot_is_a_varint_before_the_name() {
        let mut bytes = Vec::new();
        PlayPacket::SetDisplayObjective(SetDisplayObjective {
            slot: DisplaySlot::Sidebar,
            name: "race".into(),
        })
        .encode(&mut bytes);
        let mut expected = vec![0x62, 0x01];
        expected.extend(string("race"));
        assert_eq!(bytes, expected);

        let mut bytes = Vec::new();
        SetDisplayObjective {
            slot: DisplaySlot::SidebarTeamWhite,
            name: "".into(),
        }
        .encode(&mut bytes);
        assert_eq!(bytes, [18, 0]);
    }

    #[test]
    fn round_trips_and_rejects_unknown_slots() {
        let packet = SetDisplayObjective {
            slot: DisplaySlot::BelowName,
            name: "hp".into(),
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes);
        let mut slice = bytes.as_slice();
        assert_eq!(SetDisplayObjective::decode(&mut slice).unwrap(), packet);
        assert!(slice.is_empty());
        assert!(SetDisplayObjective::decode(&mut [19u8, 0].as_slice()).is_err());
    }
}
