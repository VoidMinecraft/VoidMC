use voidmc_codec::{Decode, Encode};

/// Clientbound Clear Dialog: closes the dialog the server opened.
#[derive(Debug, Clone, Encode, Decode)]
pub struct ClearDialog {}
