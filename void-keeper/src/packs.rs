//! The resource packs a client holds, and what it answered for each.

use std::collections::HashMap;

use uuid::Uuid;

#[derive(Debug)]
struct Pack {
    hash: String,
    statuses: Vec<i32>,
    settled: bool,
    offered_since_outage: bool,
}

#[derive(Debug, Default)]
pub struct PackBook {
    packs: HashMap<Uuid, Pack>,
}

enum Answer {
    Pending,
    Held,
    Lost,
}

/// A client keeps a pack it loaded, and answers `Declined` again to every push once it declined
/// one. Any other end leaves it without the pack: a push of the same one must reach it again.
fn answer(status: i32) -> Answer {
    match status {
        3 | 4 => Answer::Pending,
        0 | 1 => Answer::Held,
        _ => Answer::Lost,
    }
}

impl PackBook {
    /// What the client would answer to this push, if it holds this very pack already.
    /// A pack without a hash is never the same one: the client does not cache it either.
    pub fn offer(&mut self, id: Uuid, hash: &str) -> Option<Vec<i32>> {
        if let Some(pack) = self.packs.get_mut(&id)
            && pack.settled
            && pack.hash == hash
            && !hash.is_empty()
        {
            pack.offered_since_outage = true;
            return Some(pack.statuses.clone());
        }
        self.packs.insert(
            id,
            Pack {
                hash: hash.to_string(),
                statuses: Vec::new(),
                settled: false,
                offered_since_outage: true,
            },
        );
        None
    }

    pub fn answered(&mut self, id: Uuid, status: i32) {
        let Some(pack) = self.packs.get_mut(&id) else {
            return;
        };
        match answer(status) {
            Answer::Pending => pack.statuses.push(status),
            Answer::Held => {
                pack.statuses.push(status);
                pack.settled = true;
            }
            Answer::Lost => {
                self.packs.remove(&id);
            }
        }
    }

    pub fn popped(&mut self, id: Option<Uuid>) {
        match id {
            Some(id) => {
                self.packs.remove(&id);
            }
            None => self.packs.clear(),
        }
    }

    /// Whether the client is still downloading or applying a pack it was offered.
    pub fn awaits_client(&self) -> bool {
        self.packs.values().any(|pack| !pack.settled)
    }

    pub fn outage(&mut self) {
        for pack in self.packs.values_mut() {
            pack.offered_since_outage = false;
        }
    }

    /// The packs the new server did not offer: the client must let go of them.
    pub fn take_forsaken(&mut self) -> Vec<Uuid> {
        let forsaken: Vec<Uuid> = self
            .packs
            .iter()
            .filter(|(_, pack)| !pack.offered_since_outage)
            .map(|(id, _)| *id)
            .collect();
        for id in &forsaken {
            self.packs.remove(id);
        }
        forsaken
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCEPTED: i32 = 3;
    const DOWNLOADED: i32 = 4;
    const LOADED: i32 = 0;

    fn id() -> Uuid {
        Uuid::from_u128(7)
    }

    #[test]
    fn a_settled_pack_is_answered_as_the_client_answered() {
        let mut book = PackBook::default();
        assert_eq!(book.offer(id(), "abc"), None);
        for status in [ACCEPTED, DOWNLOADED, LOADED] {
            book.answered(id(), status);
        }
        assert_eq!(
            book.offer(id(), "abc"),
            Some(vec![ACCEPTED, DOWNLOADED, LOADED])
        );
    }

    #[test]
    fn a_new_hash_goes_to_the_client() {
        let mut book = PackBook::default();
        book.offer(id(), "abc");
        book.answered(id(), LOADED);
        assert_eq!(book.offer(id(), "def"), None);
        assert_eq!(book.offer(id(), "def"), None);
    }

    #[test]
    fn a_pack_still_downloading_is_not_settled() {
        let mut book = PackBook::default();
        book.offer(id(), "abc");
        book.answered(id(), ACCEPTED);
        assert_eq!(book.offer(id(), "abc"), None);
    }

    #[test]
    fn a_pack_without_a_hash_is_never_the_same() {
        let mut book = PackBook::default();
        book.offer(id(), "");
        book.answered(id(), LOADED);
        assert_eq!(book.offer(id(), ""), None);
    }

    #[test]
    fn a_pack_the_client_failed_to_load_goes_to_the_client_again() {
        for failure in [2, 5, 6, 7] {
            let mut book = PackBook::default();
            book.offer(id(), "abc");
            book.answered(id(), ACCEPTED);
            book.answered(id(), failure);
            assert!(!book.awaits_client(), "status {failure}");
            assert_eq!(book.offer(id(), "abc"), None, "status {failure}");
        }
    }

    #[test]
    fn a_declined_pack_is_declined_again_in_the_name_of_the_client() {
        let mut book = PackBook::default();
        book.offer(id(), "abc");
        book.answered(id(), 1);
        assert_eq!(book.offer(id(), "abc"), Some(vec![1]));
    }

    #[test]
    fn a_pack_is_awaited_until_the_client_settles_it() {
        let mut book = PackBook::default();
        assert!(!book.awaits_client());
        book.offer(id(), "abc");
        book.answered(id(), ACCEPTED);
        assert!(book.awaits_client());
        book.answered(id(), LOADED);
        assert!(!book.awaits_client());
    }

    #[test]
    fn a_pack_the_server_no_longer_offers_is_forsaken() {
        let mut book = PackBook::default();
        book.offer(id(), "abc");
        book.answered(id(), LOADED);
        let kept = Uuid::from_u128(8);
        book.offer(kept, "def");
        book.answered(kept, LOADED);
        book.outage();
        book.offer(kept, "def");
        assert_eq!(book.take_forsaken(), vec![id()]);
        assert_eq!(book.offer(kept, "def"), Some(vec![LOADED]));
        assert_eq!(book.offer(id(), "abc"), None);
    }
}
