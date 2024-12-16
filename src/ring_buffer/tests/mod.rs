use crate::ring_buffer::RingBuffer;

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
