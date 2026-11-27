// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Nodes owned by pointer, in zeroed memory, and across threads.  Run
//! under Miri to check that a node freed once it leaves is never touched
//! again, and that the threads never race.

use core::array;
use core::mem::zeroed;
use core::ptr::NonNull;
use std::sync::Mutex;
use std::thread;
use tailq::{Counted, Link, TailQueue};

/// A node on one queue.
struct Node {
    key: u32,
    link: Link,
}

tailq::adapter!(NodeLink = Node { link });

/// A queue of nodes owned by pointer.
type Queue = TailQueue<'static, NodeLink>;

/// The same, counted.
type CountedQueue = TailQueue<'static, NodeLink, Counted>;

/// Returns a node on the heap, owned by the pointer.
fn boxed(key: u32) -> NonNull<Node> {
    NonNull::from(Box::leak(Box::new(Node {
        key,
        link: Link::new(),
    })))
}

/// Frees a node `boxed` returned, and returns its key.
///
/// # Safety
///
/// The node must be on no queue, and nothing may use it afterwards.
unsafe fn free(node: NonNull<Node>) -> u32 {
    // SAFETY: the caller hands over the node `boxed` allocated.
    let owned = unsafe { Box::from_raw(node.as_ptr()) };
    assert!(!owned.link.is_linked(), "the node left its queue");
    owned.key
}

#[test]
fn nodes_freed_as_they_leave() {
    let mut queue = CountedQueue::new();
    for key in 0..6 {
        // SAFETY: each node is fresh, on no queue, and owned by the queue
        // until it leaves.
        unsafe { queue.push_back_ptr(boxed(key)) };
    }
    let mut freed = Vec::new();
    // SAFETY: each node is freed once it has left, and never used again.
    unsafe {
        freed.extend(queue.pop_front_ptr().map(|node| free(node)));
        freed.extend(queue.pop_back_ptr().map(|node| free(node)));
        let mut cursor = queue.cursor_front_mut();
        cursor.move_next();
        freed.extend(cursor.remove_current_ptr().map(|node| free(node)));
        let spare = boxed(9);
        freed.extend(cursor.replace_current_ptr(spare).map(|node| free(node)));
        let last = queue.back_ptr();
        if let Some(node) = last {
            queue.remove_ptr(node);
            freed.push(free(node));
        }
    }
    assert_eq!(freed, [0, 5, 2, 3, 4]);
    let left: Vec<u32> = queue.iter().map(|node| node.key).collect();
    assert_eq!((left, queue.len()), (vec![1, 9], 2));
    let mut rest = Vec::new();
    while let Some(node) = queue.pop_front_ptr() {
        // SAFETY: as above, for the nodes left.
        rest.push(unsafe { free(node) });
    }
    assert_eq!(rest, [1, 9]);
}

#[test]
fn zeroed_heads_and_nodes_work() {
    // SAFETY: a node of all zero bytes has key 0 and a fresh link.
    let mut nodes = unsafe { zeroed::<[Node; 4]>() };
    for (node, key) in nodes.iter_mut().zip(1..) {
        node.key = key;
    }
    // SAFETY: a head of all zero bytes is an empty queue.
    let mut queue = unsafe { zeroed::<TailQueue<'_, NodeLink>>() };
    // SAFETY: as above, counted.
    let mut counted = unsafe { zeroed::<TailQueue<'_, NodeLink, Counted>>() };
    let (left, right) = nodes.split_at_mut(2);
    queue.extend(left.iter_mut());
    counted.extend(right.iter_mut());
    queue
        .cursor_front_mut()
        .insert_after(match counted.pop_back() {
            Some(node) => node,
            None => return,
        });
    assert_eq!(counted.len(), 1);
    let mut all: TailQueue<'_, NodeLink, Counted> = TailQueue::new();
    all.extend(queue.pop_front());
    all.append(&mut counted);
    let keys: Vec<u32> =
        all.iter().chain(&queue).map(|node| node.key).collect();
    assert_eq!(keys, [1, 3, 4, 2]);
    assert_eq!(all.len(), 2);
}

#[test]
fn a_head_behind_a_static_lock_serves_threads() {
    static QUEUE: Mutex<Queue> = Mutex::new(Queue::new());
    thread::scope(|scope| {
        for lane in 0..3 {
            drop(scope.spawn(move || {
                for step in 0..4 {
                    let node = boxed(lane * 10 + step);
                    if let Ok(mut queue) = QUEUE.lock() {
                        // SAFETY: the node is fresh and on no queue, and
                        // the queue owns it until it leaves.
                        unsafe { queue.push_back_ptr(node) };
                    }
                }
            }));
        }
    });
    let mut keys = Vec::new();
    if let Ok(mut queue) = QUEUE.lock() {
        while let Some(node) = queue.pop_front_ptr() {
            // SAFETY: each node is freed once it has left.
            keys.push(unsafe { free(node) });
        }
    }
    keys.sort_unstable();
    let expected: Vec<u32> = (0..3)
        .flat_map(|lane| (0..4).map(move |step| lane * 10 + step))
        .collect();
    assert_eq!(keys, expected);
}

#[test]
fn shared_heads_are_read_from_threads() {
    let mut nodes: Vec<Node> = (0..5)
        .map(|key| Node {
            key,
            link: Link::new(),
        })
        .collect();
    let queue: TailQueue<'_, NodeLink> = nodes.iter_mut().collect();
    let sums: Vec<u32> = thread::scope(|scope| {
        let readers: [_; 3] = array::from_fn(|_| {
            scope.spawn(|| queue.iter().rev().map(|node| node.key).sum::<u32>())
        });
        readers
            .into_iter()
            .filter_map(|reader| reader.join().ok())
            .collect()
    });
    assert_eq!(sums, [10, 10, 10]);
}

#[test]
fn heads_debug_and_default() {
    let mut node = Node {
        key: 1,
        link: Link::new(),
    };
    let mut queue = TailQueue::<'_, NodeLink, Counted>::default();
    assert!(
        queue.is_empty() && format!("{queue:?}").contains("count: Counted(0)")
    );
    queue.push_back(&mut node);
    let shown =
        format!("{queue:?} {:?} {:?}", queue.iter(), queue.cursor_front());
    assert!(
        shown.contains("count: Counted(1)") && shown.contains("Cursor"),
        "{shown}"
    );
    assert!(
        format!("{:?}", Link::default()).contains("prev: Cell { value: None }")
    );
}
