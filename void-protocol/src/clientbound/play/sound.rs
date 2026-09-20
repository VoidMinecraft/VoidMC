use voidmc_codec::{Decode, DecodeError, Decoder, Encode, VarI32};

/// `Holder<SoundEvent>`: a registry id (`id + 1` on the wire) or an inline
/// event (`0`, identifier, optional fixed range).
#[derive(Debug, Clone, PartialEq)]
pub enum SoundEvent {
    Registry(i32),
    Inline {
        sound_id: String,
        fixed_range: Option<f32>,
    },
}

impl Encode for SoundEvent {
    fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            SoundEvent::Registry(id) => VarI32(id + 1).encode(buf),
            SoundEvent::Inline {
                sound_id,
                fixed_range,
            } => {
                VarI32(0).encode(buf);
                sound_id.encode(buf);
                fixed_range.encode(buf);
            }
        }
    }
}

impl Decode for SoundEvent {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        Ok(match decoder.decode::<VarI32>()?.0 {
            0 => SoundEvent::Inline {
                sound_id: decoder.decode()?,
                fixed_range: decoder.decode()?,
            },
            id if id > 0 => SoundEvent::Registry(id - 1),
            _ => return Err(DecodeError::InvalidLength),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum SoundSource {
    #[default]
    Master = 0,
    Music = 1,
    Records = 2,
    Weather = 3,
    Blocks = 4,
    Hostile = 5,
    Neutral = 6,
    Players = 7,
    Ambient = 8,
    Voice = 9,
    Ui = 10,
}

/// Positions are fixed point with 3 fractional bits (`coord * 8`).
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct SoundEffect {
    pub sound: SoundEvent,
    pub source: SoundSource,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub volume: f32,
    pub pitch: f32,
    pub seed: i64,
}

impl SoundEffect {
    pub const LOCATION_ACCURACY: f64 = 8.0;

    pub fn fixed_point(coordinate: f64) -> i32 {
        (coordinate * Self::LOCATION_ACCURACY) as i32
    }
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct EntitySoundEffect {
    pub sound: SoundEvent,
    pub source: SoundSource,
    #[codec(varint32)]
    pub entity_id: i32,
    pub volume: f32,
    pub pitch: f32,
    pub seed: i64,
}

/// Both `None` stops every sound.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StopSound {
    pub source: Option<SoundSource>,
    pub sound: Option<String>,
}

impl Encode for StopSound {
    fn encode(&self, buf: &mut Vec<u8>) {
        let flags = self.source.is_some() as u8 | (self.sound.is_some() as u8) << 1;
        flags.encode(buf);
        if let Some(source) = &self.source {
            source.encode(buf);
        }
        if let Some(sound) = &self.sound {
            sound.encode(buf);
        }
    }
}

impl Decode for StopSound {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let flags = decoder.decode::<u8>()?;
        let source = if flags & 0x01 != 0 {
            Some(decoder.decode()?)
        } else {
            None
        };
        let sound = if flags & 0x02 != 0 {
            Some(decoder.decode()?)
        } else {
            None
        };
        Ok(StopSound { source, sound })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::PlayPacket;

    fn identifier(id: &str) -> Vec<u8> {
        let mut bytes = vec![id.len() as u8];
        bytes.extend(id.as_bytes());
        bytes
    }

    fn effect(sound: SoundEvent) -> SoundEffect {
        SoundEffect {
            sound,
            source: SoundSource::Players,
            x: SoundEffect::fixed_point(1.5),
            y: SoundEffect::fixed_point(64.0),
            z: SoundEffect::fixed_point(-0.25),
            volume: 1.0,
            pitch: 1.5,
            seed: 42,
        }
    }

    fn effect_tail() -> Vec<u8> {
        let mut bytes = vec![7];
        bytes.extend(12i32.to_be_bytes());
        bytes.extend(512i32.to_be_bytes());
        bytes.extend((-2i32).to_be_bytes());
        bytes.extend(1.0f32.to_be_bytes());
        bytes.extend(1.5f32.to_be_bytes());
        bytes.extend(42i64.to_be_bytes());
        bytes
    }

    #[test]
    fn registry_holder_is_id_plus_one_after_packet_id() {
        let mut bytes = Vec::new();
        PlayPacket::SoundEffect(effect(SoundEvent::Registry(0))).encode(&mut bytes);
        let mut expected = vec![0x75, 1];
        expected.extend(effect_tail());
        assert_eq!(bytes, expected);

        let mut bytes = Vec::new();
        SoundEvent::Registry(300).encode(&mut bytes);
        assert_eq!(bytes, [0xAD, 0x02]);
    }

    #[test]
    fn inline_holder_writes_identifier_and_optional_range() {
        let mut bytes = Vec::new();
        PlayPacket::SoundEffect(effect(SoundEvent::Inline {
            sound_id: "myserver:ui/ding".into(),
            fixed_range: Some(32.0),
        }))
        .encode(&mut bytes);
        let mut expected = vec![0x75, 0];
        expected.extend(identifier("myserver:ui/ding"));
        expected.push(1);
        expected.extend(32.0f32.to_be_bytes());
        expected.extend(effect_tail());
        assert_eq!(bytes, expected);

        let mut bytes = Vec::new();
        SoundEvent::Inline {
            sound_id: "a:b".into(),
            fixed_range: None,
        }
        .encode(&mut bytes);
        assert_eq!(bytes, [0, 3, b'a', b':', b'b', 0]);
    }

    #[test]
    fn entity_sound_layout() {
        let mut bytes = Vec::new();
        PlayPacket::EntitySoundEffect(EntitySoundEffect {
            sound: SoundEvent::Registry(9),
            source: SoundSource::Ui,
            entity_id: 300,
            volume: 0.5,
            pitch: 2.0,
            seed: -1,
        })
        .encode(&mut bytes);
        let mut expected = vec![0x74, 10, 10, 0xAC, 0x02];
        expected.extend(0.5f32.to_be_bytes());
        expected.extend(2.0f32.to_be_bytes());
        expected.extend((-1i64).to_be_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn stop_sound_writes_all_four_flag_combinations() {
        let cases = [
            (StopSound::default(), vec![0x77, 0]),
            (
                StopSound {
                    source: Some(SoundSource::Music),
                    sound: None,
                },
                vec![0x77, 1, 1],
            ),
            (
                StopSound {
                    source: None,
                    sound: Some("a:b".into()),
                },
                [vec![0x77, 2], identifier("a:b")].concat(),
            ),
            (
                StopSound {
                    source: Some(SoundSource::Weather),
                    sound: Some("a:b".into()),
                },
                [vec![0x77, 3, 3], identifier("a:b")].concat(),
            ),
        ];
        for (packet, expected) in cases {
            let mut bytes = Vec::new();
            PlayPacket::StopSound(packet).encode(&mut bytes);
            assert_eq!(bytes, expected);
        }
    }

    #[test]
    fn every_packet_roundtrips() {
        fn roundtrip<T: Encode + Decode + PartialEq + std::fmt::Debug>(value: T) {
            let mut bytes = Vec::new();
            value.encode(&mut bytes);
            let mut slice = bytes.as_slice();
            assert_eq!(T::decode(&mut slice).unwrap(), value);
            assert!(slice.is_empty());
        }
        roundtrip(effect(SoundEvent::Registry(1901)));
        roundtrip(effect(SoundEvent::Inline {
            sound_id: "x:y".into(),
            fixed_range: Some(8.0),
        }));
        roundtrip(effect(SoundEvent::Inline {
            sound_id: "x:y".into(),
            fixed_range: None,
        }));
        roundtrip(EntitySoundEffect {
            sound: SoundEvent::Registry(0),
            source: SoundSource::Voice,
            entity_id: 1,
            volume: 1.0,
            pitch: 1.0,
            seed: 0,
        });
        roundtrip(StopSound {
            source: Some(SoundSource::Records),
            sound: Some("m:n".into()),
        });
        roundtrip(StopSound::default());
    }

    #[test]
    fn negative_holder_id_is_rejected() {
        let bytes = [0xFF, 0xFF, 0xFF, 0xFF, 0x0F];
        assert!(SoundEvent::decode(&mut bytes.as_slice()).is_err());
    }
}
