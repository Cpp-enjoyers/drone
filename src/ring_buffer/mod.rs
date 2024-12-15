#![allow(unused)]

#[cfg(test)]
mod tests;

use std::collections::VecDeque;

#[derive(Debug)]
pub struct RingBuffer<T> {
    buff: VecDeque<T>,
    size: usize,
}

impl<T> RingBuffer<T> {
    #[inline]
    pub fn with_capacity(size: usize) -> Self {
        Self {
            buff: if size < 0x400 {
                VecDeque::with_capacity(size)
            } else {
                VecDeque::new()
            },
            size,
        }
    }

    #[inline]
    pub fn insert(&mut self, e: T) -> Option<T> {
        let mut ret: Option<T> = None;
        if self.is_full() {
            ret = self.buff.pop_front();
        }

        self.buff.push_back(e);
        ret
    }

    #[inline]
    pub fn pop(&mut self) -> Option<T> {
        self.buff.pop_front()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buff.is_empty()
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.buff.len() == self.size
    }
}

impl<T: PartialEq> RingBuffer<T> {
    pub fn contains(&self, e: &T) -> bool {
        self.buff.contains(e)
    }
}
