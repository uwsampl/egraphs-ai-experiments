//! A basic double-ended queue where variables popped off the front aren't
//! deleted, and can be restored in a `backtrack` operation.
//!
//! We use this data-structure to incrementally maintain partial assignments of
//! nodes to classes.

use std::cmp;

#[derive(Debug)]
pub(crate) struct BacktrackQueue<T> {
    data: Vec<T>,
    front: usize,
}

#[derive(Debug)]
pub(crate) struct QueueSnapshot {
    front: usize,
    back: usize,
}

impl<T> Default for BacktrackQueue<T> {
    fn default() -> Self {
        Self {
            data: Vec::new(),
            front: 0,
        }
    }
}

impl<T: Clone> BacktrackQueue<T> {
    /// Push a new element to the back of the queue.
    pub fn push_back(&mut self, elem: T) {
        self.data.push(elem);
    }

    /// Pop an element from the front of the queue.
    pub fn pop_front(&mut self) -> Option<T> {
        if self.front < self.data.len() {
            let elem = self.data[self.front].clone();
            self.front += 1;
            Some(elem)
        } else {
            None
        }
    }

    /// Return a snapshot of the queue that can be passed  to `restore`.
    pub fn snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            front: self.front,
            back: self.data.len(),
        }
    }

    pub fn restore(&mut self, snap: &QueueSnapshot) {
        self.front = snap.front;
        self.data.truncate(snap.back);
    }

    /// Pop an element from the front of the queue.
    pub fn front(&self) -> Option<&T> {
        self.data.get(self.front)
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data[cmp::min(self.front, self.data.len())..].iter()
    }
}
