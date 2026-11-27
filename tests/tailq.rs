// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Behaviour of the tail queue through its public API.  Run under Miri to
//! check the pointer accesses as well as the results.

#![expect(
    clippy::tests_outside_test_module,
    reason = "an integration test crate is itself the test module"
)]

use core::convert::Infallible;
use core::marker::PhantomData;
use core::mem::{offset_of, size_of, swap, take, zeroed};
use core::ptr::NonNull;
use tailq::{Adapter, Counted, Link, TailQueue};

/// A node on one queue.
struct Node {
    key: u32,
    link: Link,
}

tailq::adapter!(NodeLink = Node { link });

/// A queue of nodes.
type Queue<'nodes> = TailQueue<'nodes, NodeLink>;

/// The counted queue through the same field.
type CountedQueue<'nodes> = TailQueue<'nodes, NodeLink, Counted>;

const fn node(key: u32) -> Node {
    Node {
        key,
        link: Link::new(),
    }
}

/// Returns the keys front to back.
fn keys(queue: &Queue<'_>) -> Vec<u32> {
    queue.iter().map(|node| node.key).collect()
}

/// Returns the keys back to front.
fn keys_rev(queue: &Queue<'_>) -> Vec<u32> {
    queue.iter().rev().map(|node| node.key).collect()
}

/// Returns the keys front to back, read through a cursor.
fn cursor_keys(queue: &Queue<'_>) -> Vec<u32> {
    let mut seen = Vec::new();
    let mut cursor = queue.cursor_front();
    while let Some(node) = cursor.current() {
        seen.push(node.key);
        cursor.move_next();
    }
    seen
}

#[test]
fn empty_queue_has_no_ends() {
    let queue = Queue::new();
    assert!(queue.is_empty());
    assert!(queue.front().is_none());
    assert!(queue.back().is_none());
    assert_eq!(queue.iter().next().map(|node| node.key), None);
    assert_eq!(queue.iter().next_back().map(|node| node.key), None);
    assert!(queue.cursor_front().current().is_none());
    assert!(queue.cursor_back().current().is_none());
}

#[test]
fn pushes_order_both_ends() {
    let (mut one, mut two, mut three) = (node(1), node(2), node(3));
    let mut queue = Queue::new();
    queue.push_back(&mut two);
    queue.push_front(&mut one);
    queue.push_back(&mut three);
    assert!(!queue.is_empty());
    assert_eq!(queue.front().map(|node| node.key), Some(1));
    assert_eq!(queue.back().map(|node| node.key), Some(3));
    assert_eq!(keys(&queue), [1, 2, 3]);
    assert_eq!(keys_rev(&queue), [3, 2, 1]);
    assert_eq!(cursor_keys(&queue), [1, 2, 3]);
}

#[test]
fn iterator_ends_meet_once() {
    let mut nodes: Vec<Node> = (0..5).map(node).collect();
    let mut queue = Queue::new();
    for node in &mut nodes {
        queue.push_back(node);
    }
    let mut iter = queue.iter();
    let mut seen = Vec::new();
    while let (Some(front), back) = (iter.next(), iter.next_back()) {
        seen.push(front.key);
        if let Some(last) = back {
            seen.push(last.key);
        }
    }
    assert_eq!(seen, [0, 4, 1, 3, 2]);
    assert!(iter.next().is_none());
    assert!(iter.next_back().is_none());
}

#[test]
fn draining_from_the_front_hands_nodes_back() {
    let mut nodes: Vec<Node> = (0..4).map(node).collect();
    let mut queue = Queue::new();
    for node in &mut nodes {
        queue.push_back(node);
    }
    let mut drained = Vec::new();
    while let Some(node) = queue.cursor_front_mut().remove_current() {
        node.key = node.key.wrapping_add(10);
        drained.push(node.key);
    }
    assert!(queue.is_empty());
    assert_eq!(drained, [10, 11, 12, 13]);
}

#[test]
fn cursor_moves_through_the_ghost() {
    let (mut one, mut two) = (node(1), node(2));
    let mut queue = Queue::new();
    queue.push_back(&mut one);
    queue.push_back(&mut two);

    let mut cursor = queue.cursor_back();
    assert_eq!(cursor.current().map(|node| node.key), Some(2));
    assert_eq!(cursor.peek_next().map(|node| node.key), None);
    assert_eq!(cursor.peek_prev().map(|node| node.key), Some(1));
    cursor.move_next();
    assert!(cursor.current().is_none());
    assert_eq!(cursor.peek_next().map(|node| node.key), Some(1));
    assert_eq!(cursor.peek_prev().map(|node| node.key), Some(2));
    cursor.move_next();
    assert_eq!(cursor.current().map(|node| node.key), Some(1));
    assert_eq!(cursor.peek_prev().map(|node| node.key), None);
    cursor.move_prev();
    assert!(cursor.current().is_none());
    cursor.move_prev();
    assert_eq!(cursor.current().map(|node| node.key), Some(2));
}

#[test]
fn cursor_inserts_around_nodes_and_the_ghost() {
    let (mut one, mut two, mut three) = (node(1), node(2), node(3));
    let (mut four, mut five) = (node(4), node(5));
    let mut queue = Queue::new();
    {
        let mut cursor = queue.cursor_front_mut();
        cursor.insert_after(&mut three);
        cursor.insert_before(&mut five);
        cursor.move_next();
        assert_eq!(cursor.current().map(|node| node.key), Some(3));
        cursor.insert_before(&mut one);
        cursor.insert_after(&mut four);
        cursor.move_prev();
        cursor.insert_after(&mut two);
        assert_eq!(cursor.peek_next().map(|node| node.key), Some(2));
    }
    assert_eq!(keys(&queue), [1, 2, 3, 4, 5]);
    assert_eq!(keys_rev(&queue), [5, 4, 3, 2, 1]);
}

#[test]
fn removing_the_back_keeps_the_tail() {
    let (mut one, mut two, mut three) = (node(1), node(2), node(3));
    let mut queue = Queue::new();
    queue.push_back(&mut one);
    queue.push_back(&mut two);
    let removed = queue
        .cursor_back_mut()
        .remove_current()
        .map(|node| node.key);
    assert_eq!(removed, Some(2));
    assert_eq!(queue.back().map(|node| node.key), Some(1));
    queue.push_back(&mut three);
    assert_eq!(keys(&queue), [1, 3]);
    assert_eq!(keys_rev(&queue), [3, 1]);
}

#[test]
fn removing_moves_the_cursor_on() {
    let mut nodes: Vec<Node> = (0..4).map(node).collect();
    let mut queue = Queue::new();
    for node in &mut nodes {
        queue.push_back(node);
    }
    {
        let mut cursor = queue.cursor_front_mut();
        cursor.move_next();
        let removed = cursor.remove_current().map(|node| node.key);
        assert_eq!(removed, Some(1));
        assert_eq!(cursor.current().map(|node| node.key), Some(2));
        cursor.move_next();
        cursor.move_next();
        assert!(cursor.current().is_none());
        assert!(cursor.remove_current().is_none());
    }
    assert_eq!(keys(&queue), [0, 2, 3]);
    assert_eq!(keys_rev(&queue), [3, 2, 0]);
}

#[test]
fn replace_swaps_the_node_in_place() {
    let (mut one, mut two, mut three) = (node(1), node(2), node(3));
    let (mut middle, mut last, mut spare) = (node(20), node(30), node(99));
    let mut queue = Queue::new();
    queue.push_back(&mut one);
    queue.push_back(&mut two);
    queue.push_back(&mut three);
    {
        let mut cursor = queue.cursor_front_mut();
        cursor.move_next();
        let second = cursor
            .replace_current(&mut middle)
            .map(|node| node.key)
            .map_err(|node| node.key);
        assert_eq!(second, Ok(2));
        assert_eq!(cursor.current().map(|node| node.key), Some(20));
        cursor.move_next();
        let third = cursor
            .replace_current(&mut last)
            .map(|node| node.key)
            .map_err(|node| node.key);
        assert_eq!(third, Ok(3));
        cursor.move_next();
        let refused = cursor
            .replace_current(&mut spare)
            .map(|node| node.key)
            .map_err(|node| node.key);
        assert_eq!(refused, Err(99));
    }
    assert_eq!(keys(&queue), [1, 20, 30]);
    assert_eq!(keys_rev(&queue), [30, 20, 1]);
}

#[test]
fn append_moves_every_node() {
    let mut left: Vec<Node> = (0..3).map(node).collect();
    let mut right: Vec<Node> = (3..6).map(node).collect();
    let mut first = Queue::new();
    let mut second = Queue::new();

    first.append(&mut second);
    assert!(first.is_empty());
    assert!(second.is_empty());

    for node in &mut right {
        second.push_back(node);
    }
    first.append(&mut second);
    assert!(second.is_empty());
    assert_eq!(keys(&first), [3, 4, 5]);

    for node in &mut left {
        second.push_back(node);
    }
    second.append(&mut first);
    assert!(first.is_empty());
    assert_eq!(keys(&second), [0, 1, 2, 3, 4, 5]);
    assert_eq!(keys_rev(&second), [5, 4, 3, 2, 1, 0]);

    second.append(&mut first);
    assert_eq!(keys(&second), [0, 1, 2, 3, 4, 5]);
}

#[test]
fn emptied_donor_takes_new_nodes() {
    let (mut one, mut two) = (node(1), node(2));
    let mut first = Queue::new();
    let mut second = Queue::new();
    second.push_back(&mut one);
    first.append(&mut second);
    second.push_back(&mut two);
    assert_eq!(keys(&first), [1]);
    assert_eq!(keys(&second), [2]);
    assert_eq!(keys_rev(&second), [2]);
}

#[test]
fn nodes_by_address_leave_and_rejoin() {
    let mut nodes: Vec<Node> = (0..5).map(node).collect();
    let base = nodes.as_mut_ptr();
    // SAFETY: every index is in bounds.
    let at = |i: usize| unsafe { NonNull::new_unchecked(base.add(i)) };
    let mut queue = Queue::new();
    for i in 0..5 {
        // SAFETY: the nodes outlive the queue, nothing else reaches them
        // while they are on it, and each is on no queue.
        unsafe { queue.push_back_ptr(at(i)) };
    }
    // SAFETY: each node is on this queue when it is removed, and on no
    // queue when it is pushed again.
    unsafe {
        queue.remove_ptr(at(2));
        queue.remove_ptr(at(0));
        queue.remove_ptr(at(4));
        assert_eq!(keys(&queue), [1, 3]);
        assert_eq!(keys_rev(&queue), [3, 1]);
        queue.push_front_ptr(at(4));
        queue.push_back_ptr(at(0));
        queue.remove_ptr(at(1));
        queue.remove_ptr(at(3));
        queue.push_back_ptr(at(2));
    }
    assert_eq!(keys(&queue), [4, 0, 2]);
    assert_eq!(keys_rev(&queue), [2, 0, 4]);
}

#[test]
fn clear_lets_nodes_be_pushed_again() {
    let mut nodes: Vec<Node> = (0..3).map(node).collect();
    let base = nodes.as_mut_ptr();
    // SAFETY: every index is in bounds.
    let at = |i: usize| unsafe { NonNull::new_unchecked(base.add(i)) };
    let mut queue = Queue::new();
    // SAFETY: the nodes outlive the queue, nothing else reaches them
    // while they are on it, and `clear` takes them all off before they
    // are pushed again.
    unsafe {
        for i in 0..3 {
            queue.push_back_ptr(at(i));
        }
        queue.clear();
        assert!(queue.is_empty());
        for i in 0..3 {
            queue.push_front_ptr(at(i));
        }
    }
    assert_eq!(keys(&queue), [2, 1, 0]);
}

#[test]
fn cursor_from_address_reaches_the_node() {
    let mut nodes: Vec<Node> = (0..5).map(node).collect();
    let base = nodes.as_mut_ptr();
    // SAFETY: every index is in bounds.
    let at = |i: usize| unsafe { NonNull::new_unchecked(base.add(i)) };
    let mut queue = Queue::new();
    for i in 0..4 {
        // SAFETY: the nodes outlive the queue, nothing else reaches them
        // while they are on it, and each is on no queue.
        unsafe { queue.push_back_ptr(at(i)) };
    }
    {
        // SAFETY: the node at index 2 is on this queue.
        let mut cursor = unsafe { queue.cursor_mut_from_ptr(at(2)) };
        assert_eq!(cursor.peek_prev().map(|node| node.key), Some(1));
        let removed = cursor.remove_current().map(|node| node.key);
        assert_eq!(removed, Some(2));
        assert_eq!(cursor.current().map(|node| node.key), Some(3));
        // SAFETY: the node at index 2 has left the queue.
        let old = unsafe { cursor.replace_current_ptr(at(2)) };
        assert_eq!(old, Some(at(3)));
        // SAFETY: the node at index 3 has left the queue.
        unsafe { cursor.insert_before_ptr(at(3)) };
        cursor.move_next();
        assert!(cursor.current().is_none());
        // SAFETY: the node at index 4 is on no queue.
        assert!(unsafe { cursor.replace_current_ptr(at(4)) }.is_none());
    }
    assert_eq!(keys(&queue), [0, 1, 3, 2]);
    assert_eq!(keys_rev(&queue), [2, 3, 1, 0]);
}

#[test]
fn current_ptr_writes_through_the_push_access() {
    let (mut one, mut two) = (node(1), node(2));
    let front = {
        let mut queue = Queue::new();
        queue.push_back(&mut one);
        queue.push_back(&mut two);
        {
            let mut cursor = queue.cursor_front_mut();
            while let Some(at) = cursor.current_ptr() {
                // SAFETY: the node was pushed as `&mut`, so its pointer
                // may write the key, and no `&Node` is alive meanwhile.
                unsafe {
                    (*at.as_ptr()).key = (*at.as_ptr()).key.wrapping_mul(7);
                }
                cursor.move_next();
            }
        }
        assert_eq!(keys(&queue), [7, 14]);
        queue.cursor_front().current_ptr()
    };
    assert_eq!(front.map(NonNull::as_ptr), Some(&raw mut one));
    assert_eq!((one.key, two.key), (7, 14));
}

/// A node on two queues at once, one link each.
struct Shared {
    key: u32,
    ready: Link,
    all: Link,
}

tailq::adapter!(ReadyLink = Shared { ready });
tailq::adapter!(AllLink = Shared { all });

/// The nodes ready to go.
type Ready<'nodes> = TailQueue<'nodes, ReadyLink>;
/// Every node.
type All<'nodes> = TailQueue<'nodes, AllLink>;

#[test]
fn one_node_rides_two_queues() {
    let mut nodes: Vec<Shared> = (0..4)
        .map(|key| Shared {
            key,
            ready: Link::new(),
            all: Link::new(),
        })
        .collect();
    let base = nodes.as_mut_ptr();
    // SAFETY: every index is in bounds.
    let at = |i: usize| unsafe { NonNull::new_unchecked(base.add(i)) };
    let mut ready = Ready::new();
    let mut all = All::new();
    // SAFETY: the nodes outlive both queues and are reached only through
    // them, and each link joins one queue.
    unsafe {
        for i in 0..4 {
            all.push_back_ptr(at(i));
        }
        ready.push_front_ptr(at(1));
        ready.push_front_ptr(at(3));
    }
    let all_keys: Vec<u32> = all.iter().map(|node| node.key).collect();
    let ready_keys: Vec<u32> = ready.iter().map(|node| node.key).collect();
    assert_eq!(all_keys, [0, 1, 2, 3]);
    assert_eq!(ready_keys, [3, 1]);
}

trait Platform {
    const NAME: &'static str;
}

/// A platform for the generic node.
#[derive(Debug)]
struct Pc;

impl Platform for Pc {
    const NAME: &'static str = "pc";
}

/// The record a generic node embeds its link in.
struct Sched {
    link: Link,
}

/// A node generic over its platform, linked through a nested field.
struct Thread<P: Platform> {
    id: u32,
    platform: PhantomData<P>,
    sched: Sched,
}

/// Links a generic thread through its scheduling record, by hand.
struct ThreadLink<P>(Infallible, PhantomData<P>);

// SAFETY: both directions step by the offset of `sched.link`, a `Link`,
// from the pointer they are given.
unsafe impl<P: Platform> Adapter for ThreadLink<P> {
    type Node = Thread<P>;

    unsafe fn link(node: NonNull<Thread<P>>) -> NonNull<Link> {
        // SAFETY: `node` points at a live thread.
        unsafe { NonNull::new_unchecked(&raw mut (*node.as_ptr()).sched.link) }
    }

    unsafe fn node(link: NonNull<Link>) -> NonNull<Thread<P>> {
        // SAFETY: `link` is the link of a live thread.
        unsafe { link.byte_sub(offset_of!(Thread<P>, sched.link)).cast() }
    }
}

#[test]
fn hand_written_adapter_links_a_generic_node() {
    let mut threads: Vec<Thread<Pc>> = (0..3)
        .map(|id| Thread {
            id,
            platform: PhantomData,
            sched: Sched { link: Link::new() },
        })
        .collect();
    let mut queue = TailQueue::<ThreadLink<Pc>>::new();
    for thread in &mut threads {
        queue.push_front(thread);
    }
    let ids: Vec<u32> = queue.iter().map(|thread| thread.id).collect();
    assert_eq!(ids, [2, 1, 0]);
    assert_eq!(Pc::NAME, "pc");
}

tailq::adapter!(
    SchedLink<P> = Thread<P> { sched.link } where P: Platform
);

#[test]
fn adapter_links_a_generic_node_through_a_nested_field() {
    let mut threads: Vec<Thread<Pc>> = (0..3)
        .map(|id| Thread {
            id,
            platform: PhantomData,
            sched: Sched { link: Link::new() },
        })
        .collect();
    let mut queue = TailQueue::<SchedLink<Pc>>::new();
    for thread in &mut threads {
        queue.push_back(thread);
    }
    let ids: Vec<u32> = queue.iter().rev().map(|thread| thread.id).collect();
    assert_eq!(ids, [2, 1, 0]);
}

#[test]
fn heads_in_an_array_stay_apart() {
    let mut nodes: Vec<Node> = (0..6).map(node).collect();
    let mut heads = [const { Queue::new() }; 3];
    for round in nodes.chunks_mut(3) {
        for (node, lane) in round.iter_mut().zip(&mut heads) {
            lane.push_back(node);
        }
    }
    let lanes: Vec<Vec<u32>> = heads.iter().map(keys).collect();
    assert_eq!(lanes, [[0, 3], [1, 4], [2, 5]]);
}

#[test]
fn full_heads_move() {
    let mut nodes: Vec<Node> = (0..4).map(node).collect();
    let mut queue = Queue::new();
    for node in &mut nodes {
        queue.push_back(node);
    }
    let mut other = Queue::new();
    swap(&mut queue, &mut other);
    assert!(queue.is_empty());
    let mut boxed = Box::new(take(&mut other));
    assert_eq!(keys(&boxed), [0, 1, 2, 3]);
    let first = boxed.pop_front().map(|node| node.key);
    assert_eq!(first, Some(0));
    let mut heads = vec![*boxed];
    heads.push(Queue::new());
    let last = heads.first_mut().and_then(Queue::pop_back);
    assert_eq!(last.map(|node| node.key), Some(3));
    let moved = heads.swap_remove(0);
    assert_eq!(keys(&moved), [1, 2]);
    assert_eq!(keys_rev(&moved), [2, 1]);
}

#[test]
fn heads_and_links_cross_threads() {
    const fn send_sync<T>()
    where
        T: Send + Sync,
    {
    }
    send_sync::<Link>();
    send_sync::<Queue<'static>>();
}

#[test]
fn zeroed_links_and_heads_are_fresh() {
    // SAFETY: a link of all zero bytes is a fresh one.
    let link = unsafe { zeroed::<Link>() };
    assert!(!link.is_linked());
    // SAFETY: a head of all zero bytes is an empty queue.
    let queue = unsafe { zeroed::<Queue<'static>>() };
    assert!(queue.is_empty());
    // SAFETY: as above, for a counted head.
    let counted = unsafe { zeroed::<CountedQueue<'static>>() };
    assert_eq!(counted.len(), 0);
}

#[test]
fn heads_are_two_words_or_three_counted() {
    assert_eq!(size_of::<Queue<'static>>(), 2 * size_of::<usize>());
    assert_eq!(size_of::<CountedQueue<'static>>(), 3 * size_of::<usize>());
}

#[test]
fn links_know_when_they_are_on_a_queue() {
    let (mut one, mut two, mut three) = (node(1), node(2), node(3));
    let mut spare = node(4);
    let mut queue = Queue::new();
    queue.push_back(&mut one);
    queue.push_back(&mut two);
    queue.push_back(&mut three);
    assert!(queue.iter().all(|node| node.link.is_linked()));

    let popped = queue.pop_front();
    assert!(popped.is_some_and(|node| !node.link.is_linked()));
    let replaced = {
        let mut cursor = queue.cursor_front_mut();
        cursor.replace_current(&mut spare)
    };
    assert!(replaced.is_ok_and(|node| !node.link.is_linked()));
    let removed = queue.cursor_back_mut().remove_current();
    assert!(removed.is_some_and(|node| !node.link.is_linked()));
    assert_eq!(keys(&queue), [4]);
}

#[test]
fn clear_and_drop_unmark_every_link() {
    let mut nodes: Vec<Node> = (0..3).map(node).collect();
    let base = nodes.as_mut_ptr();
    // SAFETY: every index is in bounds.
    let at = |i: usize| unsafe { NonNull::new_unchecked(base.add(i)) };
    // SAFETY: the node at `i` is live; nothing writes it meanwhile.
    let linked = |i: usize| unsafe { at(i).as_ref() }.link.is_linked();
    {
        let mut queue = Queue::new();
        // SAFETY: the nodes outlive the queue and are reached only
        // through it, and each is on no queue when pushed.
        unsafe {
            queue.push_back_ptr(at(0));
            queue.push_back_ptr(at(1));
        }
        queue.clear();
        assert!(queue.is_empty());
        assert!(!linked(0) && !linked(1));
        // SAFETY: as above; `clear` took both nodes off.
        unsafe {
            queue.push_back_ptr(at(1));
            queue.push_back_ptr(at(2));
        }
        assert!(linked(1) && linked(2));
    }
    assert!(!linked(0) && !linked(1) && !linked(2));
}

#[test]
fn counted_queue_keeps_its_length() {
    let mut nodes: Vec<Node> = (0..6).map(node).collect();
    let base = nodes.as_mut_ptr();
    // SAFETY: every index is in bounds.
    let at = |i: usize| unsafe { NonNull::new_unchecked(base.add(i)) };
    let mut queue = CountedQueue::new();
    let mut other = CountedQueue::new();
    // SAFETY: the nodes outlive both queues and are reached only through
    // them; each is pushed while on no queue and removed while on `queue`.
    unsafe {
        queue.push_back_ptr(at(0));
        queue.push_front_ptr(at(1));
        queue.cursor_front_mut().insert_after_ptr(at(2));
        queue.cursor_back_mut().insert_before_ptr(at(3));
        assert_eq!(queue.len(), 4);
        let replaced = queue.cursor_front_mut().replace_current_ptr(at(4));
        assert_eq!(replaced, Some(at(1)));
        assert_eq!(queue.len(), 4);
        queue.remove_ptr(at(2));
        assert_eq!(queue.len(), 3);
        other.push_back_ptr(at(5));
    }
    queue.append(&mut other);
    assert_eq!((queue.len(), other.len()), (4, 0));
    assert_eq!(queue.iter().count(), 4);
    assert!(queue.pop_back_ptr().is_some());
    assert_eq!(queue.len(), 3);
    queue.clear();
    assert_eq!(queue.len(), 0);
}

#[test]
fn pops_take_both_ends() {
    let mut nodes: Vec<Node> = (0..4).map(node).collect();
    let mut queue = Queue::new();
    for node in &mut nodes {
        queue.push_back(node);
    }
    let front = queue.pop_front().map(|node| node.key);
    let back = queue.pop_back().map(|node| node.key);
    assert_eq!((front, back), (Some(0), Some(3)));
    let front_ptr = queue.pop_front_ptr();
    let back_ptr = queue.pop_back_ptr();
    // SAFETY: the popped nodes are live and on no queue.
    let keys_popped = unsafe {
        (
            front_ptr.map(|node| node.as_ref().key),
            back_ptr.map(|node| node.as_ref().key),
        )
    };
    assert_eq!(keys_popped, (Some(1), Some(2)));
    assert!(queue.pop_front_ptr().is_none());
    assert!(queue.pop_back().is_none());
}

#[test]
fn pointers_from_the_queue_write_the_nodes() {
    let mut nodes: Vec<Node> = (0..4).map(node).collect();
    let base = nodes.as_mut_ptr();
    // SAFETY: every index is in bounds.
    let at = |i: usize| unsafe { NonNull::new_unchecked(base.add(i)) };
    let mut queue = Queue::new();
    for i in 0..4 {
        // SAFETY: the nodes outlive the queue, nothing else reaches them
        // while they are on it, and each is on no queue.
        unsafe { queue.push_back_ptr(at(i)) };
    }
    assert_eq!(queue.front_ptr(), Some(at(0)));
    assert_eq!(queue.back_ptr(), Some(at(3)));
    for node in queue.iter_ptr() {
        // SAFETY: the node was pushed by pointer, so the pointer the
        // queue hands back may write its key; no `&Node` is alive.
        unsafe { (*node.as_ptr()).key = (*node.as_ptr()).key.wrapping_add(10) };
    }
    for node in queue.iter_ptr().rev() {
        // SAFETY: as above.
        unsafe { (*node.as_ptr()).key = (*node.as_ptr()).key.wrapping_mul(2) };
    }
    assert_eq!(keys(&queue), [20, 22, 24, 26]);

    // SAFETY: the node at index 2 is on this queue.
    let third = unsafe { queue.cursor_from_ptr(at(2)) };
    assert_eq!(third.current_ptr(), Some(at(2)));
    assert_eq!(third.peek_next_ptr(), Some(at(3)));
    assert_eq!(third.peek_prev_ptr(), Some(at(1)));
    // SAFETY: the node at index 0 is on this queue.
    let first = unsafe { queue.cursor_from_ptr(at(0)) };
    assert_eq!(first.peek_prev_ptr(), None);

    {
        // SAFETY: the node at index 1 is on this queue.
        let mut cursor = unsafe { queue.cursor_mut_from_ptr(at(1)) };
        assert_eq!(cursor.peek_next_ptr(), Some(at(2)));
        assert_eq!(cursor.remove_current_ptr(), Some(at(1)));
        assert_eq!(cursor.current_ptr(), Some(at(2)));
        assert_eq!(cursor.peek_prev_ptr(), Some(at(0)));
    }
    assert_eq!(keys(&queue), [20, 24, 26]);
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "tail queue: the link is already on a queue")]
fn a_linked_node_cannot_join_again() {
    let mut nodes: Vec<Node> = (0..1).map(node).collect();
    let base = nodes.as_mut_ptr();
    let mut queue = Queue::new();
    let mut other = Queue::new();
    // SAFETY: the node is live and outlives both queues; the second push
    // is the misuse under test, caught before it writes anything.
    unsafe {
        let at = NonNull::new_unchecked(base);
        queue.push_back_ptr(at);
        other.push_back_ptr(at);
    }
}

/// What a payload node carries besides its link.
struct Info {
    key: u32,
    hits: u32,
}

/// A node whose payload a queue lends mutably.
struct Entry {
    link: Link,
    info: Info,
}

tailq::adapter!(EntryLink = Entry { link, mut info: Info });

const fn entry(key: u32) -> Entry {
    Entry {
        link: Link::new(),
        info: Info { key, hits: 0 },
    }
}

#[test]
fn payloads_are_lent_mutably() {
    let mut entries: Vec<Entry> = (0..4).map(entry).collect();
    let mut queue = TailQueue::<EntryLink>::new();
    for entry in &mut entries {
        queue.push_back(entry);
    }
    // Every payload borrowed at once, while the iterator walks the links.
    let all: Vec<&mut Info> = queue.iter_mut().collect();
    for (info, hits) in all.into_iter().zip(10..) {
        info.hits = hits;
    }
    for info in queue.iter_mut().rev() {
        info.hits = info.hits.wrapping_mul(2);
    }
    if let Some(info) = queue.front_mut() {
        info.key = 100;
    }
    if let Some(info) = queue.back_mut() {
        info.key = 103;
    }
    {
        let mut cursor = queue.cursor_front_mut();
        cursor.move_next();
        if let Some(info) = cursor.current_mut() {
            info.key = 101;
        }
        cursor.move_prev();
        cursor.move_prev();
        assert!(cursor.current_mut().is_none());
    }
    for info in &mut queue {
        info.hits = info.hits.wrapping_add(1);
    }
    let seen: Vec<(u32, u32)> = queue
        .iter()
        .map(|entry| (entry.info.key, entry.info.hits))
        .collect();
    assert_eq!(seen, [(100, 21), (101, 23), (2, 25), (103, 27)]);

    let mut ends = queue.iter_mut();
    let mut met = Vec::new();
    while let (Some(front), back) = (ends.next(), ends.next_back()) {
        met.push(front.key);
        if let Some(info) = back {
            met.push(info.key);
        }
    }
    assert_eq!(met, [100, 103, 101, 2]);
    assert!(queue.iter().all(|entry| entry.link.is_linked()));
}
