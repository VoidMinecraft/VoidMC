use voidmc_codec::{Decode, Encode};

/// `ClockNetworkState` for one `Holder<WorldClock>` (a synced-registry index).
/// A `rate` of `0.0` pauses the client's clock.
#[derive(Debug, Clone, Copy, PartialEq, Encode, Decode)]
pub struct ClockUpdate {
    #[codec(varint32)]
    pub clock: i32,
    #[codec(varint64)]
    pub total_ticks: i64,
    pub partial_tick: f32,
    pub rate: f32,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct SetTime {
    pub game_time: i64,
    pub clocks: Vec<ClockUpdate>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn matches_paper_layout() {
        let mut buf = Vec::new();
        SetTime {
            game_time: 0x0102_0304_0506_0708,
            clocks: vec![ClockUpdate {
                clock: 1,
                total_ticks: 300,
                partial_tick: 0.5,
                rate: 1.0,
            }],
        }
        .encode(&mut buf);
        let mut expected = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        expected.push(1);
        expected.push(1);
        expected.extend_from_slice(&[0xAC, 0x02]);
        expected.extend_from_slice(&0.5f32.to_be_bytes());
        expected.extend_from_slice(&1.0f32.to_be_bytes());
        assert_eq!(buf, expected);

        let mut slice = buf.as_slice();
        let decoded = SetTime::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(decoded.game_time, 0x0102_0304_0506_0708);
        assert_eq!(decoded.clocks.len(), 1);
        assert_eq!(decoded.clocks[0].total_ticks, 300);
        assert_eq!(decoded.clocks[0].rate, 1.0);
    }

    #[test]
    fn paused_clock_has_zero_rate_and_negative_ticks_use_ten_byte_varlong() {
        let mut buf = Vec::new();
        ClockUpdate {
            clock: 0,
            total_ticks: -1,
            partial_tick: 0.0,
            rate: 0.0,
        }
        .encode(&mut buf);
        let mut expected = vec![0u8];
        expected.extend_from_slice(&[0xFF; 9]);
        expected.push(0x01);
        expected.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(buf, expected);
    }

    #[test]
    fn empty_clock_map_is_game_time_only() {
        let mut buf = Vec::new();
        PlayPacket::SetTime(SetTime {
            game_time: 20,
            clocks: Vec::new(),
        })
        .encode(&mut buf);
        assert_eq!(buf, vec![0x71, 0, 0, 0, 0, 0, 0, 0, 20, 0]);
    }
}
