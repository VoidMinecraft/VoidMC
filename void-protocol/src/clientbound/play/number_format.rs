use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, DecodeError, Decoder, Encode, VarI32};
use voidmc_data::Version;

const VERSION: Version = Version::V26_1_2;
const REGISTRY: &str = "minecraft:number_format_type";

/// How a score renders next to its owner. `Styled` carries a text style
/// compound (`{color: ..}`), `Fixed` a full text component shown instead of the
/// number.
#[derive(Debug, Clone, PartialEq)]
pub enum NumberFormat {
    Blank,
    Styled(Nbt),
    Fixed(Nbt),
}

impl NumberFormat {
    fn registry_name(&self) -> &'static str {
        match self {
            NumberFormat::Blank => "minecraft:blank",
            NumberFormat::Styled(_) => "minecraft:styled",
            NumberFormat::Fixed(_) => "minecraft:fixed",
        }
    }

    fn type_id(&self) -> i32 {
        let name = self.registry_name();
        voidmc_data::protocol_registry_index(VERSION, REGISTRY, name)
            .unwrap_or_else(|| panic!("{name} is in the 26.1.2 {REGISTRY} registry"))
    }

    fn from_type_id(id: i32, decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let name = voidmc_data::protocol_registry(VERSION, REGISTRY)
            .and_then(|entries| entries.iter().find(|(_, candidate)| *candidate == id))
            .map(|(name, _)| *name)
            .ok_or(DecodeError::InvalidPacketId(u8::try_from(id).ok()))?;
        Ok(match name {
            "minecraft:blank" => NumberFormat::Blank,
            "minecraft:styled" => NumberFormat::Styled(decoder.decode()?),
            "minecraft:fixed" => NumberFormat::Fixed(decoder.decode()?),
            _ => return Err(DecodeError::InvalidPacketId(u8::try_from(id).ok())),
        })
    }
}

impl Encode for NumberFormat {
    fn encode(&self, buf: &mut Vec<u8>) {
        VarI32(self.type_id()).encode(buf);
        match self {
            NumberFormat::Blank => {}
            NumberFormat::Styled(style) => style.encode(buf),
            NumberFormat::Fixed(text) => text.encode(buf),
        }
    }
}

impl Decode for NumberFormat {
    fn decode_with(decoder: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let id = decoder.decode::<VarI32>()?.0;
        NumberFormat::from_type_id(id, decoder)
    }
}

#[cfg(test)]
pub(crate) fn text_nbt(text: &str) -> Nbt {
    use ussr_nbt::owned::Tag;

    Nbt {
        name: "".into(),
        compound: vec![("text".into(), Tag::String(text.into()))].into(),
    }
}

#[cfg(test)]
pub(crate) fn text_nbt_bytes(text: &str) -> Vec<u8> {
    [
        &[0x0A, 0x08, 0x00, 0x04][..],
        b"text",
        &[0x00, text.len() as u8],
        text.as_bytes(),
        &[0x00],
    ]
    .concat()
}

#[cfg(test)]
pub(crate) fn string(text: &str) -> Vec<u8> {
    let mut bytes = vec![text.len() as u8];
    bytes.extend(text.as_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use ussr_nbt::owned::Tag;

    use super::*;

    #[test]
    fn type_ids_come_from_the_registry_report() {
        assert_eq!(NumberFormat::Blank.type_id(), 0);
        assert_eq!(NumberFormat::Styled(text_nbt("")).type_id(), 1);
        assert_eq!(NumberFormat::Fixed(text_nbt("")).type_id(), 2);
    }

    #[test]
    fn blank_is_the_bare_type_id() {
        let mut bytes = Vec::new();
        NumberFormat::Blank.encode(&mut bytes);
        assert_eq!(bytes, [0x00]);
    }

    #[test]
    fn fixed_is_type_id_then_component() {
        let mut bytes = Vec::new();
        NumberFormat::Fixed(text_nbt("★")).encode(&mut bytes);
        let mut expected = vec![0x02];
        expected.extend(text_nbt_bytes("★"));
        assert_eq!(bytes, expected);
    }

    #[test]
    fn styled_is_type_id_then_style_compound() {
        let style = Nbt {
            name: "".into(),
            compound: vec![("color".into(), Tag::String("red".into()))].into(),
        };
        let mut bytes = Vec::new();
        NumberFormat::Styled(style).encode(&mut bytes);
        assert_eq!(
            bytes,
            [
                &[0x01, 0x0A, 0x08, 0x00, 0x05][..],
                b"color",
                &[0x00, 0x03],
                b"red",
                &[0x00]
            ]
            .concat()
        );
    }

    #[test]
    fn every_variant_round_trips_and_unknown_ids_fail() {
        for format in [
            NumberFormat::Blank,
            NumberFormat::Styled(text_nbt("s")),
            NumberFormat::Fixed(text_nbt("f")),
        ] {
            let mut bytes = Vec::new();
            format.encode(&mut bytes);
            let mut slice = bytes.as_slice();
            assert_eq!(NumberFormat::decode(&mut slice).unwrap(), format);
            assert!(slice.is_empty());
        }
        assert!(NumberFormat::decode(&mut [0x03u8].as_slice()).is_err());
    }
}
