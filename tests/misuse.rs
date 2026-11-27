// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Misuse of the `_ptr` operations that debug builds catch.  Each check
//! runs before the operation writes anything, so under Miri every test
//! here panics cleanly, with no undefined behaviour first, and the queues
//! still drop cleanly while the panic unwinds.

#![cfg(debug_assertions)]
#![expect(
    clippy::tests_outside_test_module,
    reason = "an integration test crate is itself the test module"
)]

use core::mem::ManuallyDrop;
use core::panic::AssertUnwindSafe;
use core::ptr::{self, NonNull};
use std::panic::catch_unwind;
use tailq::{Counted, Link, TailQueue};

/// A node on one queue.
struct Node {
    key: u32,
    link: Link,
}

tailq::adapter!(NodeLink = Node { link });

/// A queue of nodes.
type Queue = TailQueue<'static, NodeLink>;

/// Nodes owned by pointer, live for the whole test.
struct Nodes {
    base: *mut Node,
    len: usize,
}

impl Nodes {
    fn new(len: u32) -> Self {
        let nodes: Box<[Node]> = (0..len)
            .map(|key| Node {
                key,
                link: Link::new(),
            })
            .collect();
        let count = nodes.len();
        Self {
            base: Box::into_raw(nodes).cast(),
            len: count,
        }
    }

    /// Returns the node at `i`.
    fn at(&self, i: usize) -> NonNull<Node> {
        assert!(i < self.len, "a node of the set");
        // SAFETY: the index is in bounds.
        unsafe { NonNull::new_unchecked(self.base.add(i)) }
    }
}

impl Drop for Nodes {
    fn drop(&mut self) {
        // SAFETY: the slice came from `Box::into_raw`; the tests drop
        // their queues first.
        drop(unsafe {
            Box::from_raw(ptr::slice_from_raw_parts_mut(self.base, self.len))
        });
    }
}

#[test]
#[should_panic(expected = "tail queue: the link is already on a queue")]
fn a_linked_node_cannot_join_again() {
    let nodes = Nodes::new(1);
    let mut queue = Queue::new();
    let mut other = Queue::new();
    // SAFETY: the node is live and outlives both queues; the second push
    // is the misuse under test, caught before it writes anything.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        other.push_front_ptr(nodes.at(0));
    }
}

#[test]
#[should_panic(expected = "tail queue: the link is already on a queue")]
fn a_linked_node_cannot_be_inserted() {
    let nodes = Nodes::new(2);
    let mut queue = Queue::new();
    // SAFETY: as above; the insertion is the misuse under test.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        queue.push_back_ptr(nodes.at(1));
        queue.cursor_front_mut().insert_after_ptr(nodes.at(1));
    }
}

#[test]
#[should_panic(expected = "tail queue: the link is already on a queue")]
fn a_linked_node_cannot_replace_another() {
    let nodes = Nodes::new(2);
    let mut queue = Queue::new();
    // SAFETY: as above; the replacement is the misuse under test.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        queue.push_back_ptr(nodes.at(1));
        let replaced =
            queue.cursor_front_mut().replace_current_ptr(nodes.at(1));
        assert!(replaced.is_none(), "unreachable");
    }
}

#[test]
#[should_panic(expected = "tail queue: the link is on no queue")]
fn an_unlinked_node_cannot_be_removed() {
    let nodes = Nodes::new(2);
    let mut queue = Queue::new();
    // SAFETY: as above; removing the node on no queue is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        queue.remove_ptr(nodes.at(1));
    }
}

#[test]
#[should_panic(expected = "tail queue: the link is on no queue")]
fn a_node_cannot_be_removed_twice() {
    let nodes = Nodes::new(2);
    let mut queue = Queue::new();
    // SAFETY: as above; the second removal is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        queue.push_back_ptr(nodes.at(1));
        queue.remove_ptr(nodes.at(0));
        queue.remove_ptr(nodes.at(0));
    }
}

#[test]
#[should_panic(expected = "tail queue: the link is on no queue")]
fn no_cursor_rests_on_an_unlinked_node() {
    let nodes = Nodes::new(1);
    let queue = Queue::new();
    // SAFETY: the node is live; that it is on no queue is the misuse.
    let cursor = unsafe { queue.cursor_from_ptr(nodes.at(0)) };
    assert!(cursor.current().is_some(), "unreachable");
}

#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn the_first_node_of_another_queue_is_not_removed() {
    let nodes = Nodes::new(3);
    let mut queue = Queue::new();
    let mut other = Queue::new();
    // SAFETY: as above; removing another queue's first node is the
    // misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        other.push_back_ptr(nodes.at(1));
        other.push_back_ptr(nodes.at(2));
        queue.remove_ptr(nodes.at(1));
    }
}

#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn the_last_node_of_another_queue_is_not_removed() {
    let nodes = Nodes::new(3);
    let mut queue = Queue::new();
    let mut other = Queue::new();
    // SAFETY: as above; removing another queue's last node is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        other.push_back_ptr(nodes.at(1));
        other.push_back_ptr(nodes.at(2));
        queue.remove_ptr(nodes.at(2));
    }
}

#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn no_cursor_rests_on_the_first_node_of_another_queue() {
    let nodes = Nodes::new(2);
    let mut queue = Queue::new();
    let mut other = Queue::new();
    // SAFETY: as above; a cursor on another queue's node is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        other.push_back_ptr(nodes.at(1));
        let cursor = queue.cursor_mut_from_ptr(nodes.at(1));
        assert!(cursor.current().is_some(), "unreachable");
    }
}

/// Copies the bytes of the middle node of three to a spare, as a memcpy
/// of a linked node would, and returns the queue and the copy.
fn moved_middle(nodes: &Nodes) -> (Queue, NonNull<Node>) {
    let mut queue = Queue::new();
    // SAFETY: the nodes are live and outlive the queue, the first three
    // are on no queue, and the copy writes only the spare, which is on
    // no queue.
    unsafe {
        for i in 0..3 {
            queue.push_back_ptr(nodes.at(i));
        }
        ptr::copy_nonoverlapping(nodes.at(1).as_ptr(), nodes.at(3).as_ptr(), 1);
    }
    (queue, nodes.at(3))
}

#[test]
#[should_panic(expected = "tail queue: the next link does not point back")]
fn a_moved_node_is_not_removed() {
    let nodes = Nodes::new(4);
    let (mut queue, moved) = moved_middle(&nodes);
    // SAFETY: the copy reads as linked; that it is not on the queue is the
    // misuse.
    unsafe { queue.remove_ptr(moved) };
}

#[test]
#[should_panic(
    expected = "tail queue: the previous link does not point at this one"
)]
fn a_last_node_overwritten_by_another_is_not_popped() {
    let nodes = Nodes::new(4);
    let mut queue = Queue::new();
    let mut other = Queue::new();
    // SAFETY: the nodes are live and outlive both queues, and each is on
    // no queue when pushed.  Copying the last node of `other` over the
    // last of `queue` is the misuse: the copy's `prev` names a node whose
    // `next` is not it.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        queue.push_back_ptr(nodes.at(1));
        other.push_back_ptr(nodes.at(2));
        other.push_back_ptr(nodes.at(3));
        ptr::copy_nonoverlapping(nodes.at(3).as_ptr(), nodes.at(1).as_ptr(), 1);
    }
    assert!(queue.pop_back_ptr().is_none(), "unreachable");
}

#[test]
fn a_caught_check_leaves_the_queue_as_it_was() {
    let nodes = Nodes::new(4);
    let (mut queue, moved) = moved_middle(&nodes);
    let caught = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: as in `a_moved_node_is_not_removed`.
        unsafe { queue.remove_ptr(moved) };
    }));
    assert!(caught.is_err(), "the check fires");
    let keys: Vec<u32> = queue.iter().map(|node| node.key).collect();
    let keys_rev: Vec<u32> = queue.iter().rev().map(|node| node.key).collect();
    assert_eq!((keys, keys_rev), (vec![0, 1, 2], vec![2, 1, 0]));
}

/// A queue of three nodes, `0` to `2`, and another of three, `3` to `5`,
/// counted, so that a removal through the wrong one would show in a
/// length.
fn two_queues(nodes: &Nodes) -> (CountedQueue, CountedQueue) {
    let mut one = CountedQueue::new();
    let mut two = CountedQueue::new();
    // SAFETY: the nodes are live and outlive both queues, and each is on
    // no queue when pushed.
    unsafe {
        for i in 0..3 {
            one.push_back_ptr(nodes.at(i));
            two.push_back_ptr(nodes.at(i.wrapping_add(3)));
        }
    }
    (one, two)
}

/// A counted queue of nodes.
type CountedQueue = TailQueue<'static, NodeLink, Counted>;

#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn a_middle_node_of_another_queue_is_not_removed() {
    let nodes = Nodes::new(6);
    let (mut one, _two) = two_queues(&nodes);
    // SAFETY: the node is live; that it is on `two` is the misuse.
    unsafe { one.remove_ptr(nodes.at(4)) };
}

#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn a_middle_node_of_another_queue_is_not_removed_through_an_empty_one() {
    let nodes = Nodes::new(6);
    let (_one, _two) = two_queues(&nodes);
    let mut empty = Queue::new();
    // SAFETY: as above.
    unsafe { empty.remove_ptr(nodes.at(1)) };
}

#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn no_mutable_cursor_rests_on_a_middle_node_of_another_queue() {
    let nodes = Nodes::new(6);
    let (mut one, _two) = two_queues(&nodes);
    // SAFETY: as above; through the cursor, removing the other queue's
    // last node next would have set this queue's last into it.
    let cursor = unsafe { one.cursor_mut_from_ptr(nodes.at(4)) };
    assert!(cursor.current().is_some(), "unreachable");
}

#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn no_cursor_rests_on_a_middle_node_of_another_queue() {
    let nodes = Nodes::new(6);
    let (one, _two) = two_queues(&nodes);
    // SAFETY: as above.
    let cursor = unsafe { one.cursor_from_ptr(nodes.at(4)) };
    assert!(cursor.current().is_some(), "unreachable");
}

#[test]
fn a_caught_membership_check_leaves_both_queues_as_they_were() {
    let nodes = Nodes::new(6);
    let (mut one, two) = two_queues(&nodes);
    let caught = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: as in `a_middle_node_of_another_queue_is_not_removed`.
        unsafe { one.remove_ptr(nodes.at(4)) };
    }));
    assert!(caught.is_err(), "the check fires");
    let both = [&one, &two].map(|queue| {
        let keys: Vec<u32> = queue.iter().map(|node| node.key).collect();
        let back: Vec<u32> = queue.iter().rev().map(|node| node.key).collect();
        (keys, back, queue.len())
    });
    assert_eq!(
        both,
        [
            (vec![0, 1, 2], vec![2, 1, 0], 3),
            (vec![3, 4, 5], vec![5, 4, 3], 3)
        ]
    );
}

// Review 3, mutation testing: each test below fails when one debug check is
// left out, which no other test noticed.  Append to tests/misuse.rs, and add
// `use core::mem::ManuallyDrop;` to its imports.

/// Mutant M01: the membership walk reaches the far end of another queue's
/// back half first, and must check that end too.
#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn no_cursor_rests_on_a_node_near_the_back_of_another_queue() {
    let nodes = Nodes::new(6);
    let mut one = Queue::new();
    let mut two = Queue::new();
    // SAFETY: the nodes are live and outlive both queues, and each is on
    // no queue when pushed; a cursor on the fourth node of five of `two`
    // is the misuse.
    unsafe {
        one.push_back_ptr(nodes.at(0));
        for i in 1..6 {
            two.push_back_ptr(nodes.at(i));
        }
        let cursor = one.cursor_from_ptr(nodes.at(4));
        assert!(cursor.current().is_some(), "unreachable");
    }
}

/// A head of two nodes and a bitwise copy of it, as C code holding a copy
/// would keep; the copy is never dropped.
fn stale_copy(nodes: &Nodes) -> (Queue, ManuallyDrop<Queue>) {
    let mut queue = Queue::new();
    // SAFETY: the nodes are live and outlive the queue, and each is on no
    // queue when pushed.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        queue.push_back_ptr(nodes.at(1));
    }
    // SAFETY: a head is plain pointers, and the copy is never dropped.
    let copy = ManuallyDrop::new(unsafe { ptr::read(&raw const queue) });
    (queue, copy)
}

/// Mutant M04: a link with no next is the last of the queue that pops it.
#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn a_stale_head_does_not_pop_a_node_that_became_last() {
    let nodes = Nodes::new(2);
    let (mut queue, mut stale) = stale_copy(&nodes);
    assert!(queue.pop_back_ptr().is_some(), "two nodes");
    // The stale head's first node is now the queue's last, not its own.
    assert!(stale.pop_front_ptr().is_none(), "unreachable");
}

/// Mutant M05: a link with no previous link is the first of the queue
/// that pops it.
#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn a_stale_head_does_not_pop_a_node_that_became_first() {
    let nodes = Nodes::new(2);
    let (mut queue, mut stale) = stale_copy(&nodes);
    assert!(queue.pop_front_ptr().is_some(), "two nodes");
    assert!(stale.pop_back_ptr().is_none(), "unreachable");
}

/// Mutant M06: a head in a `ManuallyDrop` leaves its nodes marked as
/// linked, and safe code then pushing one at the back is caught.
#[test]
#[should_panic(expected = "tail queue: the link is already on a queue")]
fn a_node_a_forgotten_head_left_linked_cannot_be_pushed_at_the_back() {
    let mut node = Node {
        key: 0,
        link: Link::new(),
    };
    {
        let mut forgotten = ManuallyDrop::new(TailQueue::<'_, NodeLink>::new());
        forgotten.push_back(&mut node);
    }
    let mut queue = TailQueue::<'_, NodeLink>::new();
    queue.push_back(&mut node);
}

/// Mutant M07: linking before a node checks the new link too.
#[test]
#[should_panic(expected = "tail queue: the link is already on a queue")]
fn a_linked_node_cannot_be_inserted_before() {
    let nodes = Nodes::new(3);
    let mut queue = Queue::new();
    let mut other = Queue::new();
    // SAFETY: as above; inserting `other`'s node is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        queue.push_back_ptr(nodes.at(1));
        other.push_back_ptr(nodes.at(2));
        queue.cursor_back_mut().insert_before_ptr(nodes.at(2));
    }
}

/// Two queues of two, the last node of `queue` overwritten by the last of
/// `other`, so that its `prev` names a node whose `next` is not it; the
/// fifth node is a spare on no queue.
fn overwritten_last(nodes: &Nodes) -> (Queue, Queue) {
    let mut queue = Queue::new();
    let mut other = Queue::new();
    // SAFETY: the nodes are live and outlive both queues, and each is on
    // no queue when pushed; the copy is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        queue.push_back_ptr(nodes.at(1));
        other.push_back_ptr(nodes.at(2));
        other.push_back_ptr(nodes.at(3));
        ptr::copy_nonoverlapping(nodes.at(3).as_ptr(), nodes.at(1).as_ptr(), 1);
    }
    (queue, other)
}

/// Mutant M08: linking after a node checks that node first.
#[test]
#[should_panic(
    expected = "tail queue: the previous link does not point at this one"
)]
fn no_node_is_linked_after_an_overwritten_node() {
    let nodes = Nodes::new(5);
    let (mut queue, _other) = overwritten_last(&nodes);
    // SAFETY: the spare is live and on no queue.
    unsafe { queue.cursor_back_mut().insert_after_ptr(nodes.at(4)) };
}

/// Mutant M09: linking before a node checks that node first; without it
/// the new node is linked into `other`.
#[test]
#[should_panic(
    expected = "tail queue: the previous link does not point at this one"
)]
fn no_node_is_linked_before_an_overwritten_node() {
    let nodes = Nodes::new(5);
    let (mut queue, _other) = overwritten_last(&nodes);
    // SAFETY: as above.
    unsafe { queue.cursor_back_mut().insert_before_ptr(nodes.at(4)) };
}

/// Mutant M10: replacing a node checks it first.
#[test]
#[should_panic(
    expected = "tail queue: the previous link does not point at this one"
)]
fn an_overwritten_node_is_not_replaced() {
    let nodes = Nodes::new(5);
    let (mut queue, _other) = overwritten_last(&nodes);
    // SAFETY: as above.
    let replaced =
        unsafe { queue.cursor_back_mut().replace_current_ptr(nodes.at(4)) };
    assert!(replaced.is_none(), "unreachable");
}

/// Mutant M11: popping the front checks the first node.  The queue's walk
/// would enter `other` through the copy, so it is never dropped.
#[test]
#[should_panic(expected = "tail queue: the next link does not point back")]
fn a_first_node_overwritten_by_another_is_not_popped() {
    let nodes = Nodes::new(4);
    let mut queue = ManuallyDrop::new(Queue::new());
    let mut other = Queue::new();
    // SAFETY: the nodes are live and outlive both queues, and each is on
    // no queue when pushed.  Copying the first node of `other` over the
    // first of `queue` is the misuse: the copy's `next` names a node whose
    // `prev` is not it.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        queue.push_back_ptr(nodes.at(1));
        other.push_back_ptr(nodes.at(2));
        other.push_back_ptr(nodes.at(3));
        ptr::copy_nonoverlapping(nodes.at(2).as_ptr(), nodes.at(0).as_ptr(), 1);
    }
    assert!(queue.pop_front_ptr().is_none(), "unreachable");
}

/// Review 4: a push at the back checks the last node before it writes its
/// link, as linking after it through a cursor does.
#[test]
#[should_panic(
    expected = "tail queue: the previous link does not point at this one"
)]
fn no_node_is_pushed_after_an_overwritten_last_node() {
    let nodes = Nodes::new(5);
    let (mut queue, _other) = overwritten_last(&nodes);
    // SAFETY: the spare is live and on no queue.
    unsafe { queue.push_back_ptr(nodes.at(4)) };
}

/// Review 4: an append checks the last node of this queue before it
/// writes its link.
#[test]
#[should_panic(
    expected = "tail queue: the previous link does not point at this one"
)]
fn no_queue_is_appended_after_an_overwritten_last_node() {
    let nodes = Nodes::new(5);
    let (mut queue, _other) = overwritten_last(&nodes);
    let mut third = Queue::new();
    // SAFETY: the spare is live and on no queue.
    unsafe { third.push_back_ptr(nodes.at(4)) };
    queue.append(&mut third);
}

/// Two queues of two, the first node of `queue` overwritten by the first
/// of `other`, so that its `next` names a node whose `prev` is not it; the
/// fifth node is a spare on no queue.  `queue` is never dropped: its walk
/// would enter `other` through the copy.
fn overwritten_first(nodes: &Nodes) -> (ManuallyDrop<Queue>, Queue) {
    let mut queue = ManuallyDrop::new(Queue::new());
    let mut other = Queue::new();
    // SAFETY: the nodes are live and outlive both queues, and each is on
    // no queue when pushed; the copy is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        queue.push_back_ptr(nodes.at(1));
        other.push_back_ptr(nodes.at(2));
        other.push_back_ptr(nodes.at(3));
        ptr::copy_nonoverlapping(nodes.at(2).as_ptr(), nodes.at(0).as_ptr(), 1);
    }
    (queue, other)
}

/// Review 4: a push at the front checks the first node before it writes
/// its link.
#[test]
#[should_panic(expected = "tail queue: the next link does not point back")]
fn no_node_is_pushed_before_an_overwritten_first_node() {
    let nodes = Nodes::new(5);
    let (mut queue, _other) = overwritten_first(&nodes);
    // SAFETY: the spare is live and on no queue.
    unsafe { queue.push_front_ptr(nodes.at(4)) };
}

/// Review 4: an append checks the first node of the queue it takes before
/// it writes its link.
#[test]
#[should_panic(expected = "tail queue: the next link does not point back")]
fn no_queue_is_appended_with_an_overwritten_first_node() {
    let nodes = Nodes::new(5);
    let (mut donor, _other) = overwritten_first(&nodes);
    let mut queue = Queue::new();
    queue.append(&mut donor);
}

// Review 4, mutation testing: each test below fails for a mutant of a debug
// check that no other test noticed.  Append to tests/misuse.rs.

/// Mutant C01: a stale head that pops a node which already left is caught
/// before the check reads the leaving link's poisoned forward word.
#[test]
#[should_panic(expected = "tail queue: the link is on no queue")]
fn a_stale_head_does_not_pop_a_node_that_already_left() {
    let nodes = Nodes::new(2);
    let (mut queue, mut stale) = stale_copy(&nodes);
    assert!(queue.pop_front_ptr().is_some(), "two nodes");
    assert!(stale.pop_front_ptr().is_none(), "unreachable");
}

/// Mutant C03, and 0fd7ca5's claim: a link on no queue that the membership
/// walk meets on its way, here the first node's, zeroed in place, is caught
/// too, rather than read as linked through `unwrap_unchecked`.
#[test]
#[should_panic(expected = "tail queue: the link is on no queue")]
fn a_link_on_no_queue_met_mid_walk_is_caught() {
    let nodes = Nodes::new(3);
    let mut queue = Queue::new();
    // SAFETY: the nodes are live and outlive the queue, and each is on no
    // queue when pushed.  Zeroing the first node's link in place, as a
    // memset of the node would, is the misuse.
    unsafe {
        for i in 0..3 {
            queue.push_back_ptr(nodes.at(i));
        }
        ptr::write(&raw mut (*nodes.at(0).as_ptr()).link, Link::new());
        queue.remove_ptr(nodes.at(1));
    }
}

/// Review 5: clearing a queue checks every link before it writes any, so
/// a corrupt queue does not walk into another.  Here the first node of
/// `queue` holds the bytes of the first of `other`.
#[test]
#[should_panic(expected = "tail queue: the queue is corrupt")]
fn a_corrupt_queue_is_not_cleared() {
    let nodes = Nodes::new(5);
    let (mut queue, _other) = overwritten_first(&nodes);
    queue.clear();
}

/// Review 5: dropping a corrupt queue writes nothing, rather than walk
/// into another queue and mark its nodes as on no queue; the other queue
/// is left whole.
#[test]
fn dropping_a_corrupt_queue_leaves_another_alone() {
    let nodes = Nodes::new(5);
    let (queue, other) = overwritten_first(&nodes);
    drop(ManuallyDrop::into_inner(queue));
    let keys: Vec<u32> = other.iter().map(|node| node.key).collect();
    let keys_rev: Vec<u32> = other.iter().rev().map(|node| node.key).collect();
    assert_eq!((keys, keys_rev), (vec![2, 3], vec![3, 2]));
    assert!(other.iter().all(|node| node.link.is_linked()));
}

/// Review 5: the membership walk ahead meets a link that left a queue, its
/// forward word `NOWHERE`, and reports it rather than follow it.  Here the
/// fifth of six nodes holds the bytes of a node that left a queue.
#[test]
#[should_panic(expected = "tail queue: the link is on no queue")]
fn the_membership_walk_ahead_catches_a_link_that_left() {
    let nodes = Nodes::new(7);
    let mut queue = ManuallyDrop::new(Queue::new());
    let mut spare = Queue::new();
    // SAFETY: the nodes are live and outlive the queues, and each is on no
    // queue when pushed; copying a node that left over a linked one is the
    // misuse.
    unsafe {
        for i in 0..6 {
            queue.push_back_ptr(nodes.at(i));
        }
        spare.push_back_ptr(nodes.at(6));
        assert!(spare.pop_front_ptr().is_some());
        ptr::copy_nonoverlapping(nodes.at(6).as_ptr(), nodes.at(4).as_ptr(), 1);
        queue.remove_ptr(nodes.at(3));
    }
}

/// Review 5: clearing checks a link's forward word too.  Here the last of
/// two nodes keeps its `prev` but has the forward word of a link that left
/// a queue, `usize::MAX`, as if it had been half relinked.
#[test]
#[should_panic(expected = "tail queue: the queue is corrupt")]
fn a_queue_with_a_half_relinked_node_is_not_cleared() {
    let nodes = Nodes::new(2);
    let mut queue = Queue::new();
    // SAFETY: the nodes are live and outlive the queue, and each is on no
    // queue when pushed.  A link is `repr(C)` and starts with its forward
    // word; writing it is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        queue.push_back_ptr(nodes.at(1));
        let link = ptr::from_ref(&(*nodes.at(1).as_ptr()).link);
        link.cast::<usize>().cast_mut().write(usize::MAX);
    }
    queue.clear();
}

/// Gives the last of two nodes on `queue` the forward word of a link that
/// left a queue, `usize::MAX`, keeping its `prev`, as if it had been half
/// relinked.
///
/// # Safety
///
/// The nodes must be live and on no queue.
unsafe fn half_relinked(nodes: &Nodes, queue: &mut Queue) {
    // SAFETY: the caller's.  A link is `repr(C)` and starts with its
    // forward word; writing it is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        queue.push_back_ptr(nodes.at(1));
        let link = ptr::from_ref(&(*nodes.at(1).as_ptr()).link);
        link.cast::<usize>().cast_mut().write(usize::MAX);
    }
}

/// Review 6: the end checks of a pop, a push and the like refuse a forward
/// word that marks a link as on no queue, rather than read through it.
#[test]
#[should_panic(expected = "tail queue: the link is on no queue")]
fn a_pop_refuses_a_half_relinked_last_node() {
    let nodes = Nodes::new(2);
    let mut queue = ManuallyDrop::new(Queue::new());
    // SAFETY: the nodes are live, outlive the queue and are on no queue;
    // the pop is the misuse, caught before it writes anything.
    unsafe {
        half_relinked(&nodes, &mut queue);
        assert!(queue.pop_back_ptr().is_some(), "a node to pop");
    }
}

/// Review 6: as above, through a push after the last node.
#[test]
#[should_panic(expected = "tail queue: the link is on no queue")]
fn a_push_refuses_a_half_relinked_last_node() {
    let nodes = Nodes::new(3);
    let mut queue = ManuallyDrop::new(Queue::new());
    // SAFETY: as above.
    unsafe {
        half_relinked(&nodes, &mut queue);
        queue.push_back_ptr(nodes.at(2));
    }
}

/// Review 6: a copy of a middle link passes the membership walk, which
/// reaches the ends through its neighbours, but the queue still links the
/// original; no cursor rests on it, as no removal takes it.
#[test]
#[should_panic(
    expected = "tail queue: the previous link does not point at this one"
)]
fn no_cursor_rests_on_a_moved_node() {
    let nodes = Nodes::new(4);
    let (queue, moved) = moved_middle(&nodes);
    // SAFETY: the copy reads as linked; that it is not on the queue is the
    // misuse, caught before the cursor reads the node.
    let cursor = unsafe { queue.cursor_from_ptr(moved) };
    assert!(cursor.current().is_some(), "unreachable");
}

/// Review 6: as above, with a cursor that could write the queue.
#[test]
#[should_panic(
    expected = "tail queue: the previous link does not point at this one"
)]
fn no_mutable_cursor_rests_on_a_moved_node() {
    let nodes = Nodes::new(4);
    let (mut queue, moved) = moved_middle(&nodes);
    // SAFETY: as above.
    let mut cursor = unsafe { queue.cursor_mut_from_ptr(moved) };
    assert!(cursor.remove_current_ptr().is_some(), "unreachable");
}

/// Review 6: a replacement checks its node at the ghost too, as its Panics
/// section says, though it links nothing there.
#[test]
#[should_panic(expected = "tail queue: the link is already on a queue")]
fn a_replacement_at_the_ghost_checks_its_node() {
    let nodes = Nodes::new(2);
    let mut queue = Queue::new();
    let mut other = Queue::new();
    // SAFETY: the nodes are live, outlive the queues and are on no queue
    // when pushed; the replacement is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        other.push_back_ptr(nodes.at(1));
        let mut cursor = queue.cursor_front_mut();
        cursor.move_next();
        assert!(
            cursor.replace_current_ptr(nodes.at(1)).is_none(),
            "the ghost"
        );
    }
}

/// Review 6: as above, in safe code, for a node that a forgotten queue
/// left marked as on a queue.
#[test]
#[should_panic(expected = "tail queue: the link is already on a queue")]
fn a_safe_replacement_at_the_ghost_checks_its_node() {
    let mut stray = Node {
        key: 1,
        link: Link::new(),
    };
    let mut kept = Node {
        key: 0,
        link: Link::new(),
    };
    // Never dropped, so never marks its node as on no queue.
    let mut forgotten = ManuallyDrop::new(TailQueue::<'_, NodeLink>::new());
    forgotten.push_back(&mut stray);
    let mut queue = TailQueue::<'_, NodeLink>::new();
    queue.push_back(&mut kept);
    let mut cursor = queue.cursor_front_mut();
    cursor.move_next();
    assert!(cursor.replace_current(&mut stray).is_err(), "the ghost");
}

// Review 6, mutation testing: each test below fails for a mutant of clear's
// and drop's check that no other test noticed.  Append to tests/misuse.rs.

/// Pushes the first two nodes onto a queue, then, through a stale copy of
/// its head, the third: after the last when `after`, before the first
/// otherwise.  The queue's own links still lead from its first to its
/// last, every one but one pointing back as it should.
fn pushed_through_a_stale_head(nodes: &Nodes, after: bool) -> Queue {
    let mut queue = Queue::new();
    // SAFETY: the nodes are live and outlive the queues, and each is on no
    // queue when pushed; the copy of the head is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        queue.push_back_ptr(nodes.at(1));
        let mut stale = ManuallyDrop::new(ptr::read(&raw const queue));
        if after {
            stale.push_back_ptr(nodes.at(2));
        } else {
            stale.push_front_ptr(nodes.at(2));
        }
    }
    queue
}

/// Mutant L04: clearing checks that the walk from the first link ends at
/// the queue's last.  Here it runs on past the last into a node the stale
/// head pushed, whose link points back as it should.
#[test]
#[should_panic(expected = "tail queue: the queue is corrupt")]
fn a_queue_whose_walk_runs_past_its_last_is_not_cleared() {
    let nodes = Nodes::new(3);
    let mut queue = pushed_through_a_stale_head(&nodes, true);
    queue.clear();
}

/// Mutant L03: clearing checks that every link points back at the one
/// before it.  Here the first node's points at the node the stale head
/// pushed before it, while the walk forward still ends at the last.
#[test]
#[should_panic(expected = "tail queue: the queue is corrupt")]
fn a_queue_whose_first_points_back_at_another_node_is_not_cleared() {
    let nodes = Nodes::new(3);
    let mut queue = pushed_through_a_stale_head(&nodes, false);
    queue.clear();
}

/// Mutants L03 and L04: dropping either queue writes nothing, so the nodes
/// that the stale head holds stay marked as linked.
#[test]
fn dropping_a_queue_a_stale_head_changed_leaves_its_nodes_alone() {
    for after in [true, false] {
        let nodes = Nodes::new(3);
        drop(pushed_through_a_stale_head(&nodes, after));
        // SAFETY: the nodes are live, and no queue is left to write them.
        let linked = |i| unsafe { nodes.at(i).as_ref() }.link.is_linked();
        assert!((0..3).all(linked), "after: {after}");
    }
}

/// Mutant L06: a head with no first link but a last one is corrupt too, as
/// one whose first word was overwritten is, and is not cleared.
#[test]
#[should_panic(expected = "tail queue: the queue is corrupt")]
fn a_head_with_a_last_but_no_first_is_not_cleared() {
    let nodes = Nodes::new(1);
    let mut queue = Queue::new();
    // SAFETY: the node is live and outlives the queue, and is on no queue
    // when pushed.  The head is `repr(C)` and starts with its first link;
    // zeroing it, to `None`, is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        ptr::from_mut(&mut queue).cast::<usize>().write(0);
    }
    queue.clear();
}
