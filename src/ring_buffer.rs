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

#[cfg(test)]
mod tests {
    use super::RingBuffer;

    #[test]
    fn test1() {
        let mut rb: RingBuffer<(u64, u64)> = RingBuffer::with_capacity(64);
        rb.insert((0, 0));
        rb.insert((1, 0));
        rb.insert((2, 0));
        rb.insert((3, 0));
        rb.insert((4, 0));
        rb.insert((5, 0));
        rb.insert((6, 0));

        assert!(rb.contains(&(0, 0)));
        assert!(rb.contains(&(6, 0)));
        assert!(rb.contains(&(4, 0)));
        assert!(!rb.contains(&(0, 1)));
        assert!(!rb.contains(&(0, 2)));
        assert!(!rb.contains(&(0, 3)));
    }

    #[test]
    fn test2() {
        let mut rb: RingBuffer<(u64, u64)> = RingBuffer::with_capacity(64);
        rb.insert((0, 0));
        rb.insert((1, 0));
        rb.insert((2, 0));
        rb.insert((3, 0));
        rb.insert((4, 0));
        rb.insert((5, 0));
        rb.insert((6, 0));

        assert_eq!(rb.pop(), Some((0, 0)));
        assert_eq!(rb.pop(), Some((1, 0)));
        assert_eq!(rb.pop(), Some((2, 0)));
        assert_eq!(rb.pop(), Some((3, 0)));
        assert_eq!(rb.pop(), Some((4, 0)));
        assert_eq!(rb.pop(), Some((5, 0)));
        assert_eq!(rb.pop(), Some((6, 0)));
        assert!(rb.pop().is_none());
    }

    #[test]
    fn test3() {
        let mut rb: RingBuffer<(u64, u64)> = RingBuffer::with_capacity(64);
        for i in 0..101 {
            rb.insert((i, 0));
        }

        for i in 0..64 {
            assert_eq!(rb.pop(), Some((i + 37, 0)));
        }

        assert!(rb.pop().is_none())
    }
}
