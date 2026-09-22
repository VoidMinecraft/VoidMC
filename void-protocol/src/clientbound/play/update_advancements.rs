use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, DecodeError, Decoder, Encode};

use crate::clientbound::play::level_particles::ItemStackTemplate;

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct UpdateAdvancements {
    pub reset: bool,
    pub added: Vec<AdvancementHolder>,
    pub removed: Vec<String>,
    pub progress: Vec<AdvancementProgressEntry>,
    pub show_advancements: bool,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct AdvancementHolder {
    pub id: String,
    pub advancement: Advancement,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct Advancement {
    pub parent: Option<String>,
    pub display: Option<DisplayInfo>,
    pub requirements: Vec<Vec<String>>,
    pub sends_telemetry: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayInfo {
    pub title: Nbt,
    pub description: Nbt,
    pub icon: ItemStackTemplate,
    pub frame: AdvancementFrame,
    pub show_toast: bool,
    pub hidden: bool,
    pub background: Option<String>,
    pub x: f32,
    pub y: f32,
}

impl DisplayInfo {
    const HAS_BACKGROUND: i32 = 0x1;
    const SHOW_TOAST: i32 = 0x2;
    const HIDDEN: i32 = 0x4;
}

impl Encode for DisplayInfo {
    fn encode(&self, buf: &mut Vec<u8>) {
        self.title.encode(buf);
        self.description.encode(buf);
        self.icon.encode(buf);
        self.frame.encode(buf);
        let mut flags = 0;
        if self.background.is_some() {
            flags |= Self::HAS_BACKGROUND;
        }
        if self.show_toast {
            flags |= Self::SHOW_TOAST;
        }
        if self.hidden {
            flags |= Self::HIDDEN;
        }
        flags.encode(buf);
        if let Some(background) = &self.background {
            background.encode(buf);
        }
        self.x.encode(buf);
        self.y.encode(buf);
    }
}

impl Decode for DisplayInfo {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let title = decoder.decode::<Nbt>()?;
        let description = decoder.decode::<Nbt>()?;
        let icon = decoder.decode::<ItemStackTemplate>()?;
        let frame = decoder.decode::<AdvancementFrame>()?;
        let flags = decoder.decode::<i32>()?;
        let background = if flags & Self::HAS_BACKGROUND != 0 {
            Some(decoder.decode::<String>()?)
        } else {
            None
        };
        Ok(Self {
            title,
            description,
            icon,
            frame,
            show_toast: flags & Self::SHOW_TOAST != 0,
            hidden: flags & Self::HIDDEN != 0,
            background,
            x: decoder.decode::<f32>()?,
            y: decoder.decode::<f32>()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
#[codec(varint32)]
#[repr(i32)]
pub enum AdvancementFrame {
    #[default]
    Task = 0,
    Challenge = 1,
    Goal = 2,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct AdvancementProgressEntry {
    pub id: String,
    pub progress: AdvancementProgress,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct AdvancementProgress {
    pub criteria: Vec<CriterionProgressEntry>,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct CriterionProgressEntry {
    pub name: String,
    pub obtained_epoch_millis: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clientbound::ManualPlayPacket;
    use crate::clientbound::play::set_title_text::{text_nbt, text_nbt_bytes};

    fn utf(text: &str) -> Vec<u8> {
        [&[text.len() as u8][..], text.as_bytes()].concat()
    }

    fn toast_packet() -> UpdateAdvancements {
        UpdateAdvancements {
            reset: false,
            added: vec![AdvancementHolder {
                id: "void:toast/7".into(),
                advancement: Advancement {
                    parent: None,
                    display: Some(DisplayInfo {
                        title: text_nbt("Record"),
                        description: text_nbt("1:23"),
                        icon: ItemStackTemplate::simple(0x2A1, 1),
                        frame: AdvancementFrame::Challenge,
                        show_toast: true,
                        hidden: true,
                        background: None,
                        x: 0.0,
                        y: 0.0,
                    }),
                    requirements: vec![vec!["done".into()]],
                    sends_telemetry: false,
                },
            }],
            removed: vec![],
            progress: vec![AdvancementProgressEntry {
                id: "void:toast/7".into(),
                progress: AdvancementProgress {
                    criteria: vec![CriterionProgressEntry {
                        name: "done".into(),
                        obtained_epoch_millis: Some(0x0102_0304_0506_0708),
                    }],
                },
            }],
            show_advancements: false,
        }
    }

    fn toast_bytes() -> Vec<u8> {
        [
            &[0x00, 0x01][..],
            &utf("void:toast/7"),
            &[0x00, 0x01],
            &text_nbt_bytes("Record"),
            &text_nbt_bytes("1:23"),
            &[0xA1, 0x05, 0x01, 0x00, 0x00],
            &[0x01],
            &[0x00, 0x00, 0x00, 0x06],
            &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            &[0x01, 0x01],
            &utf("done"),
            &[0x00],
            &[0x00, 0x01],
            &utf("void:toast/7"),
            &[0x01],
            &utf("done"),
            &[0x01, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08],
            &[0x00],
        ]
        .concat()
    }

    #[test]
    fn add_matches_paper_layout() {
        let mut buf = Vec::new();
        toast_packet().encode(&mut buf);
        assert_eq!(buf, toast_bytes());

        let mut slice = buf.as_slice();
        let decoded = UpdateAdvancements::decode(&mut slice).unwrap();
        assert!(slice.is_empty());
        assert_eq!(decoded, toast_packet());
    }

    #[test]
    fn remove_matches_paper_layout() {
        let packet = UpdateAdvancements {
            reset: false,
            added: vec![],
            removed: vec!["void:toast/7".into()],
            progress: vec![],
            show_advancements: false,
        };
        let mut buf = Vec::new();
        packet.encode(&mut buf);
        assert_eq!(
            buf,
            [&[0x00, 0x00, 0x01][..], &utf("void:toast/7"), &[0x00, 0x00]].concat()
        );

        let mut slice = buf.as_slice();
        assert_eq!(UpdateAdvancements::decode(&mut slice).unwrap(), packet);
        assert!(slice.is_empty());
    }

    #[test]
    fn background_and_parent_are_flagged_and_optional() {
        let display = DisplayInfo {
            title: text_nbt("a"),
            description: text_nbt("b"),
            icon: ItemStackTemplate::simple(1, 1),
            frame: AdvancementFrame::Goal,
            show_toast: false,
            hidden: false,
            background: Some("minecraft:gui/advancements/backgrounds/stone".into()),
            x: 1.5,
            y: -2.0,
        };
        let mut buf = Vec::new();
        display.encode(&mut buf);
        let tail = &buf[buf.len() - 8..];
        assert_eq!(tail, [0x3F, 0xC0, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00]);
        let flags_at = text_nbt_bytes("a").len() + text_nbt_bytes("b").len() + 4 + 1;
        assert_eq!(&buf[flags_at..flags_at + 4], [0x00, 0x00, 0x00, 0x01]);
        assert_eq!(
            &buf[flags_at + 4..flags_at + 4 + 1 + 44],
            utf("minecraft:gui/advancements/backgrounds/stone")
        );

        let mut slice = buf.as_slice();
        assert_eq!(DisplayInfo::decode(&mut slice).unwrap(), display);
        assert!(slice.is_empty());
    }

    #[test]
    fn manual_packet_id() {
        let mut buf = Vec::new();
        ManualPlayPacket::UpdateAdvancements(toast_packet()).encode(&mut buf);
        assert_eq!(buf[..2], [0x82, 0x01]);
        assert_eq!(&buf[2..], toast_bytes());
    }
}
