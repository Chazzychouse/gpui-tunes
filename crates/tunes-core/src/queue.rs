//! Play order and cursor. Pure data, no I/O.

use crate::track::TrackId;

/// The ordered list of tracks that next/previous move through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Queue {
    order: Vec<TrackId>,
    current: Option<usize>,
}

impl Queue {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the order. The current track is kept if it is still present.
    pub fn set_order(&mut self, order: Vec<TrackId>) {
        let current_id = self.current();
        self.order = order;
        self.current = current_id.and_then(|id| self.position_of(id));
    }

    /// The current order.
    pub fn order(&self) -> &[TrackId] {
        &self.order
    }

    /// The track the cursor is on.
    pub fn current(&self) -> Option<TrackId> {
        self.current.and_then(|i| self.order.get(i).copied())
    }

    /// Move the cursor to `id`. Returns `false` if it is not in the queue.
    pub fn jump_to(&mut self, id: TrackId) -> bool {
        match self.position_of(id) {
            Some(i) => {
                self.current = Some(i);
                true
            }
            None => false,
        }
    }

    /// Advance, wrapping at the end. Starts at the front when there is no cursor.
    // Not an iterator: the cursor wraps and the queue is navigated in both directions.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<TrackId> {
        if self.order.is_empty() {
            return None;
        }
        self.current = Some(self.current.map_or(0, |i| (i + 1) % self.order.len()));
        self.current()
    }

    /// Step back, wrapping at the front. Starts at the end when there is no cursor.
    // Not an iterator: the cursor wraps and the queue is navigated in both directions.
    #[allow(clippy::should_implement_trait)]
    pub fn prev(&mut self) -> Option<TrackId> {
        if self.order.is_empty() {
            return None;
        }
        let len = self.order.len();
        self.current = Some(self.current.map_or(len - 1, |i| (i + len - 1) % len));
        self.current()
    }

    fn position_of(&self, id: TrackId) -> Option<usize> {
        self.order.iter().position(|t| *t == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[u32]) -> Vec<TrackId> {
        list.iter().map(|n| TrackId(*n)).collect()
    }

    #[test]
    fn empty_queue_yields_nothing() {
        let mut q = Queue::new();
        assert_eq!(q.current(), None);
        assert_eq!(q.next(), None);
        assert_eq!(q.prev(), None);
        assert!(!q.jump_to(TrackId(1)));
    }

    #[test]
    fn next_and_prev_wrap_around() {
        let mut q = Queue::new();
        q.set_order(ids(&[5, 6, 7]));
        assert!(q.jump_to(TrackId(6)));
        assert_eq!(q.current(), Some(TrackId(6)));
        assert_eq!(q.next(), Some(TrackId(7)));
        assert_eq!(q.next(), Some(TrackId(5)));
        assert_eq!(q.prev(), Some(TrackId(7)));
    }

    #[test]
    fn next_without_current_starts_at_front() {
        let mut q = Queue::new();
        q.set_order(ids(&[1, 2]));
        assert_eq!(q.next(), Some(TrackId(1)));
        let mut q = Queue::new();
        q.set_order(ids(&[1, 2]));
        assert_eq!(q.prev(), Some(TrackId(2)));
    }

    #[test]
    fn replacing_order_keeps_current_if_present() {
        let mut q = Queue::new();
        q.set_order(ids(&[1, 2, 3]));
        q.jump_to(TrackId(2));
        q.set_order(ids(&[3, 2]));
        assert_eq!(q.current(), Some(TrackId(2)));
        assert_eq!(q.next(), Some(TrackId(3)));
        q.set_order(ids(&[9]));
        assert_eq!(q.current(), None);
    }
}
