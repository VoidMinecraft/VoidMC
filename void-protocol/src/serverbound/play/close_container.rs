use voidmc_codec::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct CloseContainer {
    #[codec(varint32)]
    pub container_id: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_id_is_a_varint() {
        let mut slice = &[0x82u8, 0x01][..];
        assert_eq!(
            CloseContainer::decode(&mut slice).unwrap().container_id,
            130
        );
        assert!(slice.is_empty());
    }
}
