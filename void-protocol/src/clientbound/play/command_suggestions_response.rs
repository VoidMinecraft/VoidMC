use voidmc_codec::Encode;

#[derive(Debug, Clone)]
pub struct CommandSuggestionsResponse {
    pub transaction_id: i32,
    pub start: i32,
    pub length: i32,
    pub matches: Vec<String>,
}

impl Encode for CommandSuggestionsResponse {
    fn encode(&self, buf: &mut Vec<u8>) {
        voidmc_codec::VarI32(self.transaction_id).encode(buf);
        voidmc_codec::VarI32(self.start).encode(buf);
        voidmc_codec::VarI32(self.length).encode(buf);
        voidmc_codec::VarI32(self.matches.len() as i32).encode(buf);
        for m in &self.matches {
            m.encode(buf);
            false.encode(buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_id_range_and_entries_without_tooltip() {
        let packet = CommandSuggestionsResponse {
            transaction_id: 129,
            start: 4,
            length: 2,
            matches: vec!["survival".into(), "spectator".into()],
        };
        let mut buf = Vec::new();
        packet.encode(&mut buf);

        let mut expected = vec![0x81, 0x01, 4, 2, 2, 8];
        expected.extend(b"survival");
        expected.push(0);
        expected.push(9);
        expected.extend(b"spectator");
        expected.push(0);
        assert_eq!(buf, expected);
    }
}
