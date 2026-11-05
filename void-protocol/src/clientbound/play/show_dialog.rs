use ussr_nbt::owned::Nbt;
use voidmc_codec::{Decode, Encode, VarI32};

/// Registry holders use `0` for an inline value and `id + 1` for a reference.
const INLINE_HOLDER: i32 = 0;

/// Clientbound Show Dialog: opens a dialog screen, replacing any open dialog.
#[derive(Debug, Clone, Encode, Decode)]
pub struct ShowDialog {
    holder: VarI32,
    dialog: Nbt,
}

impl ShowDialog {
    /// Shows a dialog defined inline, so it needs no `minecraft:dialog` registry entry.
    pub fn inline(dialog: Nbt) -> Self {
        Self {
            holder: VarI32(INLINE_HOLDER),
            dialog,
        }
    }

    pub fn dialog(&self) -> &Nbt {
        &self.dialog
    }
}

#[cfg(test)]
mod tests {
    use ussr_nbt::owned::Tag;

    use super::*;

    #[test]
    fn an_inline_dialog_starts_with_the_direct_holder_marker() {
        let dialog = Nbt {
            name: "".into(),
            compound: vec![("type".into(), Tag::String("minecraft:notice".into()))].into(),
        };
        let mut encoded = Vec::new();
        ShowDialog::inline(dialog.clone()).encode(&mut encoded);

        let mut nbt = Vec::new();
        dialog.encode(&mut nbt);
        assert_eq!(encoded[0], 0);
        assert_eq!(&encoded[1..], nbt.as_slice());
    }
}
