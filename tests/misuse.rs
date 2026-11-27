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

use core::panic::AssertUnwindSafe;
use core::ptr::{self, NonNull};
use std::panic::catch_unwind;
use tailq::{Link, TailQueue};

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
