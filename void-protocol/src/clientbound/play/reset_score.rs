use voidmc_codec::{Decode, Encode};

/// `objective: None` resets the owner's scores in every objective.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct ResetScore {
    pub owner: String,
    pub objective: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::super::number_format::string;
    use super::*;
    use crate::clientbound::PlayPacket;

    #[test]
    fn objective_is_a_nullable_string() {
        let mut bytes = Vec::new();
        PlayPacket::ResetScore(ResetScore {
            owner: "Leo".into(),
            objective: Some("race".into()),
        })
        .encode(&mut bytes);
        let mut expected = vec![0x4F];
        expected.extend(string("Leo"));
        expected.push(0x01);
        expected.extend(string("race"));
        assert_eq!(bytes, expected);

        let mut bytes = Vec::new();
        ResetScore {
            owner: "Leo".into(),
            objective: None,
        }
        .encode(&mut bytes);
        let mut expected = string("Leo");
        expected.push(0x00);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn round_trips() {
        for objective in [Some("race".to_string()), None] {
            let packet = ResetScore {
                owner: "Leo".into(),
                objective,
            };
            let mut bytes = Vec::new();
            packet.encode(&mut bytes);
            let mut slice = bytes.as_slice();
            assert_eq!(ResetScore::decode(&mut slice).unwrap(), packet);
            assert!(slice.is_empty());
        }
    }
}
