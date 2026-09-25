use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Charges both a message slot and its payload bytes until the queued item is dropped.
pub struct Quota {
    max_items: usize,
    max_bytes: usize,
    items: AtomicUsize,
    bytes: AtomicUsize,
    high_items: AtomicUsize,
    high_bytes: AtomicUsize,
}

impl Quota {
    pub fn new(max_items: usize, max_bytes: usize) -> Arc<Self> {
        Arc::new(Self {
            max_items,
            max_bytes,
            items: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
            high_items: AtomicUsize::new(0),
            high_bytes: AtomicUsize::new(0),
        })
    }

    pub fn reserve(self: &Arc<Self>, bytes: usize) -> Option<Reservation> {
        let mut items = self.items.load(Ordering::Relaxed);
        loop {
            if items >= self.max_items {
                return None;
            }
            match self.items.compare_exchange_weak(
                items,
                items + 1,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => items = actual,
            }
        }
        let mut used = self.bytes.load(Ordering::Relaxed);
        loop {
            let Some(next) = used.checked_add(bytes) else {
                self.items.fetch_sub(1, Ordering::AcqRel);
                return None;
            };
            if next > self.max_bytes {
                self.items.fetch_sub(1, Ordering::AcqRel);
                return None;
            }
            match self
                .bytes
                .compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Relaxed)
            {
                Ok(_) => {
                    self.high_items
                        .fetch_max(self.items.load(Ordering::Relaxed), Ordering::Relaxed);
                    self.high_bytes.fetch_max(next, Ordering::Relaxed);
                    return Some(Reservation {
                        quota: Arc::clone(self),
                        bytes,
                    });
                }
                Err(actual) => used = actual,
            }
        }
    }

    pub fn high_water(&self) -> (usize, usize) {
        (
            self.high_items.load(Ordering::Relaxed),
            self.high_bytes.load(Ordering::Relaxed),
        )
    }
}

pub struct Reservation {
    quota: Arc<Quota>,
    bytes: usize,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.quota.bytes.fetch_sub(self.bytes, Ordering::AcqRel);
        self.quota.items.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforces_both_limits_and_releases_on_drop() {
        let quota = Quota::new(2, 10);
        let first = quota.reserve(6).unwrap();
        assert!(quota.reserve(5).is_none());
        let second = quota.reserve(4).unwrap();
        assert!(quota.reserve(0).is_none());
        assert_eq!(quota.high_water(), (2, 10));
        drop(first);
        assert!(quota.reserve(6).is_some());
        drop(second);
    }
}
