// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Misuse of the `_ptr` operations that debug builds catch.  Each check
//! runs before the operation writes anything, so under Miri every test
//! here panics cleanly, with no undefined behaviour first, and the queues
//! still drop cleanly while the panic unwinds.

#![cfg(debug_assertions)]

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
#[should_panic(expected = "tail queue: the link is not on this queue")]
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

/// The membership walk reaches the far end of another queue's back half
/// first, and checks that end too.
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

/// A link with no next is the last of the queue that pops it.
#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn a_stale_head_does_not_pop_a_node_that_became_last() {
    let nodes = Nodes::new(2);
    let (mut queue, mut stale) = stale_copy(&nodes);
    assert!(queue.pop_back_ptr().is_some(), "two nodes");
    // The stale head's first node is now the queue's last, not its own.
    assert!(stale.pop_front_ptr().is_none(), "unreachable");
}

/// A link with no previous link is the first of the queue that pops it.
#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn a_stale_head_does_not_pop_a_node_that_became_first() {
    let nodes = Nodes::new(2);
    let (mut queue, mut stale) = stale_copy(&nodes);
    assert!(queue.pop_front_ptr().is_some(), "two nodes");
    assert!(stale.pop_back_ptr().is_none(), "unreachable");
}

/// A head in a `ManuallyDrop` leaves its nodes marked as linked, and safe
/// code then pushing one at the back is caught.
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

/// Linking before a node checks the new link too.
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

/// Linking after a node checks that node first.
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

/// Linking before a node checks that node first; without it the new node
/// would be linked into `other`.
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

/// Replacing a node checks it first.
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

/// Popping the front checks the first node.  The queue's walk would enter
/// `other` through the copy, so it is never dropped.
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

/// A push at the back checks the last node before it writes its link, as
/// linking after it through a cursor does.
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

/// An append checks the last node of this queue before it writes its
/// link.
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

/// A push at the front checks the first node before it writes its link.
#[test]
#[should_panic(expected = "tail queue: the next link does not point back")]
fn no_node_is_pushed_before_an_overwritten_first_node() {
    let nodes = Nodes::new(5);
    let (mut queue, _other) = overwritten_first(&nodes);
    // SAFETY: the spare is live and on no queue.
    unsafe { queue.push_front_ptr(nodes.at(4)) };
}

/// An append checks the first node of the queue it takes before it writes
/// its link.
#[test]
#[should_panic(expected = "tail queue: the next link does not point back")]
fn no_queue_is_appended_with_an_overwritten_first_node() {
    let nodes = Nodes::new(5);
    let (mut donor, _other) = overwritten_first(&nodes);
    let mut queue = Queue::new();
    queue.append(&mut donor);
}

/// A stale head that pops a node which already left is caught before the
/// check reads the leaving link's poisoned forward word.
#[test]
#[should_panic(expected = "tail queue: the link is on no queue")]
fn a_stale_head_does_not_pop_a_node_that_already_left() {
    let nodes = Nodes::new(2);
    let (mut queue, mut stale) = stale_copy(&nodes);
    assert!(queue.pop_front_ptr().is_some(), "two nodes");
    assert!(stale.pop_front_ptr().is_none(), "unreachable");
}

/// The membership walk refuses the forward word of a link that left.  Here
/// a stale copy of a head of five nodes, kept after the queue popped its
/// first, removes the fourth: the walk from the front reads the poisoned
/// word of the node that left before the walk from the back meets the
/// fourth.
#[test]
#[should_panic(expected = "tail queue: the queue is corrupt")]
fn a_stale_head_whose_first_left_removes_nothing() {
    let nodes = Nodes::new(5);
    let mut queue = Queue::new();
    // SAFETY: the nodes are live and outlive the queue, and each is on no
    // queue when pushed.
    unsafe {
        for i in 0..5 {
            queue.push_back_ptr(nodes.at(i));
        }
    }
    // SAFETY: a head is plain pointers, and the copy is never dropped.
    let mut stale = ManuallyDrop::new(unsafe { ptr::read(&raw const queue) });
    assert!(queue.pop_front_ptr().is_some(), "five nodes");
    // SAFETY: the misuse: the copy is not the queue.
    unsafe { stale.remove_ptr(nodes.at(3)) };
}

/// A cursor does not step off a node that left while the cursor was in
/// use: the forward word it reads is the one debug builds fill a leaving
/// link's with.  Here code that reaches a head by pointer removes the
/// middle of three nodes under a cursor resting on it.  Under Miri,
/// Stacked Borrows refuses the cursor's borrow of the head first.
#[test]
#[cfg_attr(miri, ignore = "Stacked Borrows refuses the cursor's borrow first")]
#[should_panic(
    expected = "tail queue: the node at the cursor has left the queue"
)]
fn a_cursor_does_not_step_off_a_node_that_left() {
    let nodes = Nodes::new(3);
    let mut head = Queue::new();
    let queue = ptr::from_mut(&mut head);
    // SAFETY: the nodes are live and outlive the queue, and each is on no
    // queue when pushed; removing the middle one while the cursor is in
    // use is the misuse.
    unsafe {
        for i in 0..3 {
            (*queue).push_back_ptr(nodes.at(i));
        }
        let mut cursor = (*queue).cursor_from_ptr(nodes.at(1));
        (*queue).remove_ptr(nodes.at(1));
        cursor.move_next();
    }
}

/// Clearing a queue checks every link before it writes any, so a corrupt
/// queue does not walk into another.  Here the first node of `queue` holds
/// the bytes of the first of `other`.
#[test]
#[should_panic(expected = "tail queue: the queue is corrupt")]
fn a_corrupt_queue_is_not_cleared() {
    let nodes = Nodes::new(5);
    let (mut queue, _other) = overwritten_first(&nodes);
    queue.clear();
}

/// Clearing checks a link's forward word too.  Here the last of two nodes
/// keeps its `prev` but has the forward word of a link that left a queue,
/// `usize::MAX`, as if it had been half relinked.
#[test]
#[should_panic(expected = "tail queue: the queue is corrupt")]
fn a_queue_with_a_half_relinked_node_is_not_cleared() {
    let nodes = Nodes::new(2);
    // Never dropped: the drop would walk the corrupt queue.
    let mut queue = ManuallyDrop::new(Queue::new());
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

/// The end checks of a pop, a push and the like refuse a forward word that
/// marks a link as on no queue, rather than read through it.
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

/// As above, through a push after the last node.
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

/// A copy of a middle link reaches the queue's ends through its
/// neighbours, while the queue still links the original, so the
/// membership walk meets only the queue's own links, and not the copy: no
/// cursor rests on it, as no removal takes it.
#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn no_cursor_rests_on_a_moved_node() {
    let nodes = Nodes::new(4);
    let (queue, moved) = moved_middle(&nodes);
    // SAFETY: the copy reads as linked; that it is not on the queue is the
    // misuse, caught before the cursor reads the node.
    let cursor = unsafe { queue.cursor_from_ptr(moved) };
    assert!(cursor.current().is_some(), "unreachable");
}

/// As above, with a cursor that could write the queue.
#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn no_mutable_cursor_rests_on_a_moved_node() {
    let nodes = Nodes::new(4);
    let (mut queue, moved) = moved_middle(&nodes);
    // SAFETY: as above.
    let mut cursor = unsafe { queue.cursor_mut_from_ptr(moved) };
    assert!(cursor.remove_current_ptr().is_some(), "unreachable");
}

/// A replacement checks its node at the ghost too, as its Panics section
/// says, though it links nothing there.
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

/// As above, in safe code, for a node that a forgotten queue left marked
/// as on a queue.
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

/// Clearing checks that the walk from the first link ends at the queue's
/// last.  Here it runs on past the last into a node the stale head pushed,
/// whose link points back as it should.
#[test]
#[should_panic(expected = "tail queue: the queue is corrupt")]
fn a_queue_whose_walk_runs_past_its_last_is_not_cleared() {
    let nodes = Nodes::new(3);
    let mut queue = pushed_through_a_stale_head(&nodes, true);
    queue.clear();
}

/// Clearing checks that every link points back at the one before it.  Here
/// the first node's points at the node the stale head pushed before it,
/// while the walk forward still ends at the last.
#[test]
#[should_panic(expected = "tail queue: the queue is corrupt")]
fn a_queue_whose_first_points_back_at_another_node_is_not_cleared() {
    let nodes = Nodes::new(3);
    let mut queue = pushed_through_a_stale_head(&nodes, false);
    queue.clear();
}

/// A head with no first link but a last one is corrupt too, as one whose
/// first word was overwritten is, and is not cleared.
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

/// The end checks run both ways: a link that says it is at an end must be
/// the head's, and the link the head names as an end must have no
/// neighbour past it.  Otherwise a stale head whose last has gained a
/// successor would push after it, cutting off the queue's later nodes.
#[test]
#[should_panic(expected = "tail queue: the head's end has a neighbour")]
fn a_stale_head_does_not_push_after_a_last_that_has_a_successor() {
    let nodes = Nodes::new(4);
    // SAFETY: the nodes are live, outlive the queues and are on no queue
    // when pushed; the push through the stale head is the misuse, caught
    // before it writes anything.
    unsafe {
        let (mut queue, mut stale) = stale_copy(&nodes);
        queue.push_back_ptr(nodes.at(2));
        stale.push_back_ptr(nodes.at(3));
    }
}

/// As above, through a pop from a stale head whose first has gained a
/// predecessor.
#[test]
#[should_panic(expected = "tail queue: the head's end has a neighbour")]
fn a_stale_head_does_not_pop_a_first_that_has_a_predecessor() {
    let nodes = Nodes::new(3);
    // SAFETY: as above.
    unsafe {
        let (mut queue, mut stale) = stale_copy(&nodes);
        queue.push_front_ptr(nodes.at(2));
        assert!(stale.pop_front_ptr().is_some(), "unreachable");
    }
}

/// As above, for each end of a stale head, through a push at the front and
/// a pop at the back.
#[test]
fn a_stale_head_is_caught_at_either_end() {
    for front in [true, false] {
        let nodes = Nodes::new(4);
        let caught = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: as above.
            unsafe {
                let (mut queue, mut stale) = stale_copy(&nodes);
                if front {
                    queue.push_front_ptr(nodes.at(2));
                    stale.push_front_ptr(nodes.at(3));
                } else {
                    queue.push_back_ptr(nodes.at(2));
                    assert!(stale.pop_back_ptr().is_some(), "unreachable");
                }
            }
        }));
        assert!(caught.is_err(), "front: {front}");
    }
}

/// As above, through an append onto a stale head.
#[test]
#[should_panic(expected = "tail queue: the head's end has a neighbour")]
fn a_stale_head_takes_no_append() {
    let nodes = Nodes::new(4);
    let mut other = Queue::new();
    // SAFETY: as above.
    unsafe {
        let (mut queue, mut stale) = stale_copy(&nodes);
        queue.push_back_ptr(nodes.at(2));
        other.push_back_ptr(nodes.at(3));
        stale.append(&mut other);
    }
}

/// Pushes the one node onto a queue and zeroes the head's last link, to
/// `None`, keeping its first.
///
/// # Safety
///
/// The node must be live, outlive the queue and be on no queue.
unsafe fn first_but_no_last(nodes: &Nodes) -> Queue {
    let mut queue = Queue::new();
    // SAFETY: the caller's.  The head is `repr(C)`, its last link second;
    // zeroing it is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        ptr::from_mut(&mut queue).cast::<usize>().add(1).write(0);
    }
    queue
}

/// A head with a first link but no last is corrupt, as one with a last but
/// no first is, and is not cleared: the walk from its first must end at
/// its last, which `None` is not.
#[test]
#[should_panic(expected = "tail queue: the queue is corrupt")]
fn a_head_with_a_first_but_no_last_is_not_cleared() {
    let nodes = Nodes::new(1);
    // SAFETY: the node is live, outlives the queue and is on no queue.
    let mut queue = unsafe { first_but_no_last(&nodes) };
    queue.clear();
}

/// An append checks the other queue's last as well as its first, so a
/// stale head whose last has gained a successor does not give this queue
/// a last with a neighbour.
#[test]
#[should_panic(expected = "tail queue: the head's end has a neighbour")]
fn an_append_checks_the_other_queues_last() {
    let nodes = Nodes::new(4);
    let mut queue = Queue::new();
    // SAFETY: the nodes are live, outlive the queues and are on no queue
    // when pushed; appending the stale head is the misuse.
    unsafe {
        let (mut source, mut stale) = stale_copy(&nodes);
        source.push_back_ptr(nodes.at(2));
        queue.push_back_ptr(nodes.at(3));
        queue.append(&mut stale);
    }
}

/// An append onto itself, through a copy of its head, is refused: it
/// would make a ring of the queue, which the membership walk and
/// `contains_ptr` would never leave.
#[test]
#[should_panic(expected = "tail queue: the queue is appended to itself")]
fn a_queue_is_not_appended_to_itself() {
    let nodes = Nodes::new(2);
    let (mut queue, mut copy) = stale_copy(&nodes);
    queue.append(&mut copy);
}

/// The membership walk meets its node from the back when the links forward
/// skip it, and the cursor then refuses the node, which the link before it
/// does not point at.
#[test]
#[should_panic(
    expected = "tail queue: the previous link does not point at this one"
)]
fn no_cursor_rests_on_a_node_the_links_forward_skip() {
    let nodes = Nodes::new(5);
    let mut queue = Queue::new();
    // SAFETY: the nodes are live, outlive the queue and are on no queue
    // when pushed.  A link is `repr(C)` and starts with its forward word;
    // pointing the third's past the fourth is the misuse.
    unsafe {
        for i in 0..5 {
            queue.push_back_ptr(nodes.at(i));
        }
        let skip = ptr::from_ref(&(*nodes.at(2).as_ptr()).link);
        let past = ptr::from_ref(&(*nodes.at(4).as_ptr()).link);
        skip.cast::<*const Link>().cast_mut().write(past);
        assert!(queue.cursor_from_ptr(nodes.at(3)).current().is_some());
    }
}

/// Pushes the first two nodes onto a queue and zeroes the link of the one
/// at `i` in place, as a memset of the node would.
///
/// # Safety
///
/// The nodes must be live, outlive the queue and be on no queue.
unsafe fn zeroed_at(nodes: &Nodes, i: usize) -> Queue {
    let mut queue = Queue::new();
    // SAFETY: the caller's; zeroing the link is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        queue.push_back_ptr(nodes.at(1));
        ptr::write(&raw mut (*nodes.at(i).as_ptr()).link, Link::new());
    }
    queue
}

/// A link whose `prev` is `None`, here the first of two, zeroed in place,
/// is on no queue, and the end check says so before it holds the link's
/// missing next to the head's last.
#[test]
#[should_panic(expected = "tail queue: the link is on no queue")]
fn a_zeroed_first_link_is_not_popped() {
    let nodes = Nodes::new(2);
    // SAFETY: the nodes are live, outlive the queue and are on no queue.
    let mut queue = unsafe { zeroed_at(&nodes, 0) };
    assert!(queue.pop_front_ptr().is_some(), "unreachable");
}

/// As above, for the last of two, whose `None` the end check would
/// otherwise take for the previous link and read at address zero.
#[test]
#[should_panic(expected = "tail queue: the link is on no queue")]
fn a_zeroed_last_link_is_not_popped() {
    let nodes = Nodes::new(2);
    // SAFETY: as above.
    let mut queue = unsafe { zeroed_at(&nodes, 1) };
    assert!(queue.pop_back_ptr().is_some(), "unreachable");
}

/// A link with no next is at the back of a queue only when the head names
/// it as its last, which a head with a first but no last does not, so such
/// a head pops nothing.
#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn a_head_with_a_first_but_no_last_pops_nothing() {
    let nodes = Nodes::new(1);
    // SAFETY: the node is live, outlives the queue and is on no queue.
    let mut queue = unsafe { first_but_no_last(&nodes) };
    assert!(queue.pop_front_ptr().is_some(), "unreachable");
}

/// As above, a link whose `prev` marks it as first is at the front of a
/// queue only when the head names it as its first, which a head with a
/// last but no first does not.
#[test]
#[should_panic(expected = "tail queue: the link is not on this queue")]
fn a_head_with_a_last_but_no_first_pops_nothing() {
    let nodes = Nodes::new(1);
    let mut queue = Queue::new();
    // SAFETY: the node is live and outlives the queue, and is on no queue
    // when pushed.  The head is `repr(C)` and starts with its first link;
    // zeroing it, to `None`, is the misuse.
    unsafe {
        queue.push_back_ptr(nodes.at(0));
        ptr::from_mut(&mut queue).cast::<usize>().write(0);
    }
    assert!(queue.pop_back_ptr().is_some(), "unreachable");
}

/// An append refuses a copy of its own head by its last as well as its
/// first, so a stale copy whose first has since left does not take a copy
/// of the live head, the same chain from its second node, and make a ring
/// of it.
#[test]
#[should_panic(expected = "tail queue: the queue is appended to itself")]
fn a_stale_head_does_not_append_its_own_chain() {
    let nodes = Nodes::new(3);
    let mut queue = Queue::new();
    // SAFETY: the nodes are live, outlive the queue and are on no queue
    // when pushed; the copies are the misuse.
    unsafe {
        for i in 0..3 {
            queue.push_back_ptr(nodes.at(i));
        }
        let mut stale = ManuallyDrop::new(ptr::read(&raw const queue));
        assert!(queue.pop_front_ptr().is_some(), "a node to pop");
        let mut copy = ManuallyDrop::new(ptr::read(&raw const queue));
        stale.append(&mut copy);
    }
}

/// A lookup by address checks each link's `prev` before it follows its
/// `next`, so it ends on a ring, which misuse can make, rather than walk it
/// for ever.  Here the last node's forward word points back at the first.
#[test]
#[should_panic(expected = "tail queue: the queue is corrupt")]
fn a_lookup_ends_on_a_ring() {
    let nodes = Nodes::new(4);
    let mut queue = ManuallyDrop::new(Queue::new());
    // SAFETY: the nodes are live, outlive the queue and are on no queue
    // when pushed.  A link is `repr(C)` and starts with its forward word;
    // pointing the last's at the first is the misuse.
    unsafe {
        for i in 0..3 {
            queue.push_back_ptr(nodes.at(i));
        }
        let last = ptr::from_ref(&(*nodes.at(2).as_ptr()).link);
        let first = ptr::from_ref(&(*nodes.at(0).as_ptr()).link);
        last.cast::<*const Link>().cast_mut().write(first);
    }
    assert!(!queue.contains_ptr(nodes.at(3)), "unreachable");
}
