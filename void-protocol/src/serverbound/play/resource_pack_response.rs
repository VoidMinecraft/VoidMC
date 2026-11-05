use uuid::Uuid;
use voidmc_codec::{Decode, Encode, VarI32};

/// Serverbound Resource Pack Response: progress of a pushed resource pack.
#[derive(Debug, Encode, Decode)]
pub struct ResourcePackResponse {
    pub id: Uuid,
    action: VarI32,
}

/// What the client reports about a pack, in wire order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourcePackStatus {
    SuccessfullyLoaded,
    Declined,
    FailedDownload,
    Accepted,
    Downloaded,
    InvalidUrl,
    FailedReload,
    Discarded,
}

impl ResourcePackStatus {
    /// Whether the client will report nothing further for this pack.
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Accepted | Self::Downloaded)
    }
}

impl ResourcePackResponse {
    /// `None` when the client sent a status this protocol version does not define.
    pub fn status(&self) -> Option<ResourcePackStatus> {
        use ResourcePackStatus::*;
        [
            SuccessfullyLoaded,
            Declined,
            FailedDownload,
            Accepted,
            Downloaded,
            InvalidUrl,
            FailedReload,
            Discarded,
        ]
        .get(usize::try_from(self.action.0).ok()?)
        .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(action: i32) -> ResourcePackResponse {
        ResourcePackResponse {
            id: Uuid::nil(),
            action: VarI32(action),
        }
    }

    #[test]
    fn statuses_follow_the_wire_order() {
        assert_eq!(
            response(0).status(),
            Some(ResourcePackStatus::SuccessfullyLoaded)
        );
        assert_eq!(response(3).status(), Some(ResourcePackStatus::Accepted));
        assert_eq!(response(7).status(), Some(ResourcePackStatus::Discarded));
    }

    #[test]
    fn an_unknown_status_is_rejected() {
        assert_eq!(response(8).status(), None);
        assert_eq!(response(-1).status(), None);
    }

    #[test]
    fn only_progress_statuses_are_not_terminal() {
        assert!(!ResourcePackStatus::Accepted.is_terminal());
        assert!(!ResourcePackStatus::Downloaded.is_terminal());
        assert!(ResourcePackStatus::Declined.is_terminal());
    }
}
