// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! One test per problem a review found, named after what it pins, so that
//! the problem cannot come back unnoticed.  Run under Miri as well: several
//! of them fail only there.  The misuse that debug builds catch is in
//! `misuse.rs`, and what must not compile is in `src/compile_fail.rs`.

#![expect(
    clippy::tests_outside_test_module,
    reason = "an integration test crate is itself the test module"
)]

use core::mem::{replace, take};
#[cfg(debug_assertions)]
use core::ptr;
use core::ptr::NonNull;
use tailq::__private::{aligned, check_payload};
use tailq::{Counted, Link, TailQueue};

/// A node whose key is its payload.
struct Node {
    link: Link,
    key: u32,
}

tailq::adapter!(NodeLink = Node { link, mut key: u32 });

/// A queue of nodes.
type Queue<'nodes> = TailQueue<'nodes, NodeLink>;

/// The same, counted.
type CountedQueue<'nodes> = TailQueue<'nodes, NodeLink, Counted>;

fn nodes<const N: usize>() -> [Node; N] {
    let mut key = 0;
    [(); N].map(|()| {
        key = u32::wrapping_add(key, 1);
        Node {
            link: Link::new(),
            key,
        }
    })
}

fn keys<C>(queue: &TailQueue<'_, NodeLink, C>) -> Vec<u32>
where
    C: tailq::Count,
{
    queue.iter().map(|node| node.key).collect()
}

/// 0.1: once its last node left, a head pointed at itself, so moving an
/// "empty" head and pushing again wrote into the old place.
#[test]
fn an_emptied_head_moves_and_takes_nodes_again() {
    let mut nodes = nodes::<3>();
    let (first, rest) = nodes.split_at_mut(1);
    let mut queue: Queue<'_> = first.iter_mut().collect();
    assert!(queue.pop_back().is_some() && queue.is_empty());
    let mut moved = Box::new(take(&mut queue));
    moved.extend(rest.iter_mut());
    let mut again = replace(&mut *moved, Queue::new());
    assert!(again.pop_front().is_some());
    assert_eq!(keys(&again), [3]);
    assert_eq!(
        again.iter().rev().map(|node| node.key).collect::<Vec<_>>(),
        [3]
    );
}

/// Mutant M1 of the second review: a cursor built from a pointer must
/// reach the node through the pointer it was pushed with.  Here the
/// pointer comes from a `&Node`, which may not write the node, so Miri
/// fails this test if the cursor kept that pointer.
#[test]
fn a_cursor_from_a_read_only_pointer_hands_back_the_push_access() {
    let mut nodes = nodes::<3>();
    {
        let mut queue: Queue<'_> = nodes.iter_mut().collect();
        let second = queue.iter().nth(1).map(NonNull::from);
        assert!(second.is_some(), "three nodes");
        if let Some(read_only) = second {
            // SAFETY: the node is on this queue, and its pointer reads its
            // link.
            let mut cursor = unsafe { queue.cursor_mut_from_ptr(read_only) };
            if let Some(removed) = cursor.remove_current() {
                removed.key = 20;
            }
        }
        let first = queue.front().map(NonNull::from);
        assert!(first.is_some(), "two nodes");
        if let Some(read_only) = first {
            // SAFETY: as above.
            let pushed =
                unsafe { queue.cursor_from_ptr(read_only) }.current_ptr();
            if let Some(writable) = pushed {
                // SAFETY: the node was pushed as `&mut`, and no `&Node` is
                // alive.
                unsafe { (*writable.as_ptr()).key = 10 };
            }
        }
    }
    assert_eq!(nodes.map(|node| node.key), [10, 20, 3]);
}

/// Mutant M7 of the second review: `IterMut::last` lends the back
/// payload, also after the back end moved, and nothing once exhausted.
#[test]
fn iter_mut_last_lends_the_back_payload() {
    let mut nodes = nodes::<3>();
    {
        let mut queue: Queue<'_> = nodes.iter_mut().collect();
        if let Some(key) = queue.iter_mut().last() {
            *key = key.wrapping_add(30);
        }
        let mut iter = queue.iter_mut();
        assert!(iter.next_back().is_some());
        if let Some(key) = iter.last() {
            *key = key.wrapping_add(20);
        }
        let mut drained = queue.iter_mut();
        assert_eq!(drained.by_ref().take(3).count(), 3);
        assert!(drained.last().is_none());
    }
    assert_eq!(nodes.map(|node| node.key), [1, 22, 33]);
}

/// Mutant M4 of the second review: debug builds fill a leaving link's
/// forward word with an address no link can have, `usize::MAX`.
#[cfg(debug_assertions)]
#[test]
fn a_leaving_link_is_poisoned_in_debug_builds() {
    let mut nodes = nodes::<3>();
    {
        let mut queue: Queue<'_> = nodes.iter_mut().collect();
        assert!(queue.pop_front().is_some());
        queue.clear();
    }
    for node in &nodes {
        // SAFETY: a link is `repr(C)` and starts with its forward word, an
        // `Option<NonNull<Link>>` in a `Cell`; no queue holds the node.
        let next = unsafe {
            ptr::from_ref(&node.link)
                .cast::<Option<NonNull<u8>>>()
                .read()
        };
        assert_eq!(next.map(NonNull::addr).map(usize::from), Some(usize::MAX));
    }
}

/// Point 4 of the adoption review: a counted queue's iterators know how
/// many nodes they have left, from either end.
#[test]
fn counted_iterators_know_their_length() {
    let mut nodes = nodes::<5>();
    let mut queue: CountedQueue<'_> = nodes.iter_mut().collect();
    let mut iter = queue.iter();
    assert_eq!((iter.len(), iter.size_hint()), (5, (5, Some(5))));
    assert!(iter.next().is_some() && iter.next_back().is_some());
    assert_eq!(iter.len(), 3);
    assert_eq!(iter.clone().count(), 3);
    assert_eq!(
        iter.by_ref().map(|node| node.key).collect::<Vec<_>>(),
        [2, 3, 4]
    );
    assert_eq!((iter.len(), iter.clone().count()), (0, 0));
    let mut ptrs = queue.iter_ptr();
    assert!(ptrs.nth(3).is_some());
    assert_eq!(ptrs.len(), 1);
    let mut payloads = queue.iter_mut();
    assert!(payloads.next_back().is_some());
    assert_eq!((payloads.len(), payloads.count()), (4, 4));
    assert_eq!(queue.iter().rev().collect::<Vec<_>>().capacity(), 5);
}

/// The same for an uncounted queue, which knows only whether it has no
/// node left, one, or at least two.
#[test]
fn uncounted_iterators_bound_their_length() {
    let mut nodes = nodes::<3>();
    let mut queue: Queue<'_> = nodes.iter_mut().collect();
    let mut iter = queue.iter();
    assert_eq!(iter.size_hint(), (2, None));
    assert!(iter.next().is_some() && iter.next().is_some());
    assert_eq!(iter.size_hint(), (1, Some(1)));
    assert_eq!(iter.clone().count(), 1);
    assert!(iter.next().is_some());
    assert_eq!((iter.size_hint(), iter.count()), ((0, Some(0)), 0));
    assert_eq!(queue.iter_ptr().count(), 3);
    assert_eq!(queue.iter_mut().count(), 3);
    assert_eq!(Queue::new().iter().size_hint(), (0, Some(0)));
}

/// The walks `fold` and `rfold` run, for every iterator, visit each node
/// once in order, from a fresh iterator or a partly used one.
#[test]
fn folds_visit_every_node_once() {
    let mut nodes = nodes::<4>();
    let mut queue: Queue<'_> = nodes.iter_mut().collect();
    let push = |mut seen: Vec<u32>, key: u32| {
        seen.push(key);
        seen
    };
    let forward = queue
        .iter()
        .fold(Vec::new(), |seen, node| push(seen, node.key));
    let backward = queue
        .iter()
        .rfold(Vec::new(), |seen, node| push(seen, node.key));
    assert_eq!((forward, backward), (vec![1, 2, 3, 4], vec![4, 3, 2, 1]));
    let mut partly = queue.iter_ptr();
    assert!(partly.next().is_some() && partly.next_back().is_some());
    let none: usize = 0;
    let tally = |count: usize, _| count.wrapping_add(1);
    let middle = partly.clone().fold(none, tally);
    let middle_rev = partly.rfold(none, tally);
    assert_eq!((middle, middle_rev), (2, 2));
    for key in &mut queue {
        *key = key.wrapping_mul(10);
    }
    queue
        .iter_mut()
        .rev()
        .for_each(|key| *key = key.wrapping_add(1));
    assert_eq!(keys(&queue), [11, 21, 31, 41]);
    let empty = Queue::new();
    assert_eq!(empty.iter().fold(none, |count, _| count.wrapping_add(1)), 0);
    assert_eq!(
        empty.iter().rfold(none, |count, _| count.wrapping_add(1)),
        0
    );
}

/// Point 2 of the adoption review: `adapter!` takes lifetime and const
/// parameters, and a node type that names a lifetime.
#[test]
fn adapters_take_lifetimes_and_consts() {
    struct Named<'name, T, const N: usize> {
        name: &'name str,
        tags: [T; N],
        link: Link,
    }

    tailq::adapter!(
        NamedLink<'name, T, const N: usize> = Named<'name, T, N> {
            link,
            mut tags: [T; N],
        } where T: Copy
    );

    tailq::adapter!(StaticLink = Named<'static, u8, 0> { link });

    let text = String::from("node");
    let mut nodes = [1, 2].map(|tag: u8| Named {
        name: text.as_str(),
        tags: [tag; 3],
        link: Link::new(),
    });
    let mut queue: TailQueue<'_, NamedLink<'_, u8, 3>> =
        nodes.iter_mut().collect();
    for tags in &mut queue {
        if let Some(tag) = tags.first_mut() {
            *tag = tag.wrapping_add(10);
        }
    }
    let seen: Vec<(&str, u8)> =
        queue.iter().map(|node| (node.name, node.tags[0])).collect();
    assert_eq!(seen, [("node", 11), ("node", 12)]);
    let mut lone = Named {
        name: "static",
        tags: [],
        link: Link::new(),
    };
    let mut statics = TailQueue::<'_, StaticLink>::new();
    statics.push_back(&mut lone);
    assert_eq!(statics.front().map(|node| node.name), Some("static"));
}

/// The adoption review asked for a safe way to tell whether a queue holds
/// a node named by address, and to remove it only if so.
#[test]
fn lookups_by_address_are_safe() {
    let mut nodes = nodes::<4>();
    let base = nodes.as_mut_ptr();
    // SAFETY: every index is in bounds.
    let at = |i: usize| unsafe { NonNull::new_unchecked(base.add(i)) };
    let mut queue = TailQueue::<'static, NodeLink, Counted>::new();
    // SAFETY: the nodes outlive the queue, nothing else reaches them, and
    // each is on no queue.
    unsafe {
        queue.push_back_ptr(at(0));
        queue.push_back_ptr(at(2));
    }
    assert!(queue.contains_ptr(at(2)) && !queue.contains_ptr(at(1)));
    assert_eq!(
        queue
            .cursor_find_ptr(at(2))
            .and_then(|cursor| cursor.peek_prev_ptr()),
        Some(at(0))
    );
    assert!(queue.cursor_find_ptr(at(3)).is_none());
    let removed = queue
        .cursor_mut_find_ptr(at(0))
        .and_then(|mut cursor| cursor.remove_current_ptr());
    assert_eq!(removed, Some(at(0)));
    assert!(queue.cursor_mut_find_ptr(at(0)).is_none());
    assert_eq!(queue.len(), 1);
    queue.clear();
}

/// Coverage found the explicit `Clone` of a cursor, `IterMut::fold`, and
/// the `Debug` of a mutable cursor and of a payload iterator never run.
#[test]
fn cursors_and_iterators_clone_fold_and_print() {
    let mut nodes = nodes::<3>();
    let mut queue: Queue<'_> = nodes.iter_mut().collect();
    let cursor = queue.cursor_back();
    #[expect(
        clippy::clone_on_copy,
        reason = "the explicit Clone of a Copy cursor is what runs here"
    )]
    let copy = cursor.clone();
    assert_eq!(copy.current().map(|node| node.key), Some(3));
    let none: u32 = 0;
    let sum = queue
        .iter_mut()
        .fold(none, |total, key| total.wrapping_add(*key));
    assert_eq!(sum, 6);
    let payloads = format!("{:?}", queue.iter_mut());
    let cursor_mut = format!("{:?}", queue.cursor_front_mut());
    assert!(
        payloads.starts_with("IterMut") && cursor_mut.starts_with("CursorMut")
    );
}

/// A generic node whose adapter lends the link itself as the payload,
/// which a generic adapter checks only where it is used.
struct Wrapped<T> {
    link: Link,
    extra: T,
}

tailq::adapter!(OverLink<T> = Wrapped<T> { link, mut link: Link });

/// Coverage found that the checks `adapter!` runs at compile time never
/// ran in a test: here they run, both ways.
#[test]
fn the_compile_time_checks_answer_at_run_time() {
    assert!(aligned::<Node, Link>(0));
    assert!(!aligned::<Node, Link>(1));
    assert!(!aligned::<u8, Link>(0));
    check_payload::<NodeLink>();
    let extra: u8 = 7;
    let wrapped = Wrapped {
        link: Link::new(),
        extra,
    };
    assert!(!wrapped.link.is_linked() && wrapped.extra == 7);
}

#[test]
#[should_panic(expected = "tailq: the payload overlaps the link")]
fn an_overlapping_payload_fails_its_check() {
    check_payload::<OverLink<u8>>();
}
