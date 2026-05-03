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
            false.encode(buf); // has_tooltip = false
        }
    }
}
