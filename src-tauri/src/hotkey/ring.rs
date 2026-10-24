//! Lock-free single-producer/single-consumer ring of event codes. The hook thread pushes
//! (no allocation, no locks); the forwarder thread pops. Full ring drops the newest event.

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

const CAP: usize = 64;

pub struct Ring {
    slots: [AtomicU8; CAP],
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl Ring {
    pub const fn new() -> Self {
        Self { slots: [const { AtomicU8::new(0) }; CAP], head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }
    }

    pub fn push(&self, code: u8) -> bool {
        let t = self.tail.load(Ordering::Relaxed);
        if t.wrapping_sub(self.head.load(Ordering::Acquire)) >= CAP {
            return false;
        }
        self.slots[t % CAP].store(code, Ordering::Relaxed);
        self.tail.store(t.wrapping_add(1), Ordering::Release);
        true
    }

    pub fn pop(&self) -> Option<u8> {
        let h = self.head.load(Ordering::Relaxed);
        if h == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let v = self.slots[h % CAP].load(Ordering::Relaxed);
        self.head.store(h.wrapping_add(1), Ordering::Release);
        Some(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_wraps_and_drops_when_full() {
        let r = Ring::new();
        assert_eq!(r.pop(), None);
        for i in 0..CAP as u8 {
            assert!(r.push(i + 1));
        }
        assert!(!r.push(99), "full");
        for i in 0..CAP as u8 {
            assert_eq!(r.pop(), Some(i + 1));
        }
        assert_eq!(r.pop(), None);
        for round in 0..200u32 {
            assert!(r.push((round % 250) as u8 + 1));
            assert_eq!(r.pop(), Some((round % 250) as u8 + 1));
        }
    }
}
