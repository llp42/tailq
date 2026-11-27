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

use core::mem::size_of;
use core::mem::{replace, take};
use core::ptr;
use core::ptr::NonNull;
use tailq::__private::{aligned, check_payload, link_offset, payload_offset};
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
    assert_eq!(link_offset::<Node>(0), 0);
    assert_eq!(payload_offset::<Node, u32>(16), 16);
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

#[test]
#[should_panic(
    expected = "tailq::adapter!: the link is not aligned in every node"
)]
fn an_unaligned_link_fails_its_check() {
    assert_eq!(link_offset::<u8>(0), 0);
}

#[test]
#[should_panic(
    expected = "tailq::adapter!: the payload is not aligned in every node"
)]
fn an_unaligned_payload_fails_its_check() {
    assert_eq!(payload_offset::<Node, u32>(1), 1);
}

/// Fills a queue reached by pointer, as a head in a `static mut` is, with
/// the nodes at `base`, and returns the pointer to the queue.
///
/// # Safety
///
/// `base` must point at `N` nodes that outlive the queue, on no queue.
unsafe fn by_pointer<const N: usize>(
    head: &mut Queue<'_>,
    base: *mut Node,
) -> *mut Queue<'static> {
    let queue = ptr::from_mut(head).cast::<Queue<'static>>();
    for i in 0..N {
        // SAFETY: as the caller promises.
        unsafe { (*queue).push_back_ptr(NonNull::new_unchecked(base.add(i))) };
    }
    queue
}

/// The third review found that a walk over a head reached by pointer, a
/// `static mut` one, ran into the poisoned link of the node it had just
/// handed out when the walk was a `for_each`.  Removing what the walk just
/// handed out works from either end, through `next` and through `fold`.
#[test]
fn a_walk_by_pointer_may_remove_what_it_hands_out() {
    let ways: [u8; 4] = [0, 1, 2, 3];
    for way in ways {
        let mut nodes = nodes::<4>();
        let mut head = Queue::new();
        let mut seen = Vec::new();
        // SAFETY: the nodes outlive the queue and are on no queue; each is
        // removed once, right after the walk hands it out, and no reference
        // to the queue is alive meanwhile.
        unsafe {
            let queue = by_pointer::<4>(&mut head, nodes.as_mut_ptr());
            let mut take = |node: NonNull<Node>| {
                seen.push((*node.as_ptr()).key);
                (*queue).remove_ptr(node);
            };
            match way {
                0 => (*queue).iter_ptr().for_each(&mut take),
                1 => (*queue).iter_ptr().rev().for_each(&mut take),
                2 => {
                    for node in (*queue).iter_ptr() {
                        take(node);
                    }
                }
                _ => {
                    for node in (*queue).iter_ptr().rev() {
                        take(node);
                    }
                }
            }
            assert!((*queue).is_empty());
        }
        let expected: [u32; 4] = if way.is_multiple_of(2) {
            [1, 2, 3, 4]
        } else {
            [4, 3, 2, 1]
        };
        assert_eq!(seen, expected);
    }
}

/// The same review found that the head's `repr(C)` promised a count of a
/// word without `Counted` being `repr(transparent)`.
#[test]
fn a_count_is_one_word() {
    assert_eq!(size_of::<Counted>(), size_of::<usize>());
    assert_eq!(size_of::<CountedQueue<'static>>(), 3 * size_of::<usize>());
}

// Review 3, mutation testing: each test below fails for a mutant no other
// test noticed.  Append to tests/regressions.rs.

/// Mutants M02 and M03: the membership walk debug builds run for a node
/// named by address goes toward both ends at once, so a node next to
/// either end of a long queue costs a step or two.  A walk toward one end
/// only costs thousands of times more at the other.
#[cfg(debug_assertions)]
#[cfg_attr(miri, ignore = "times a walk")]
#[test]
fn membership_walks_toward_the_nearer_end() {
    use core::hint::black_box;
    use core::iter::repeat_with;
    use core::time::Duration;
    use std::time::Instant;

    const NODES: usize = 1 << 13;
    const CALLS: usize = 1 << 12;
    let mut nodes: Vec<Node> = repeat_with(|| Node {
        link: Link::new(),
        key: 0,
    })
    .take(NODES)
    .collect();
    let queue: Queue<'_> = nodes.iter_mut().collect();
    let time = |node: Option<NonNull<Node>>| {
        let trials: u32 = 5;
        let mut best = Duration::MAX;
        for _ in 0..trials {
            let start = Instant::now();
            for _ in 0..CALLS {
                if let Some(at) = node {
                    // SAFETY: the node is on this queue.
                    let cursor = unsafe { queue.cursor_from_ptr(at) };
                    let _: Option<NonNull<Node>> =
                        black_box(cursor.current_ptr());
                }
            }
            best = best.min(start.elapsed());
        }
        best
    };
    let near_front = time(queue.iter_ptr().nth(1));
    let near_back = time(queue.iter_ptr().nth(NODES.wrapping_sub(2)));
    let slack = Duration::from_millis(2);
    assert!(
        near_front < near_back * 50 + slack
            && near_back < near_front * 50 + slack,
        "front {near_front:?}, back {near_back:?}"
    );
}

/// Mutant M12: a cursor found by address reaches the node through the
/// pointer it was pushed with, not the one it was found by.  Here that one
/// comes from a `&Node`, which may not write the node, so Miri fails this
/// test if the cursor kept it.
#[test]
fn a_cursor_found_by_a_read_only_pointer_hands_back_the_push_access() {
    let mut nodes = nodes::<3>();
    {
        let mut queue: Queue<'_> = nodes.iter_mut().collect();
        let second = queue.iter().nth(1).map(NonNull::from);
        assert!(second.is_some(), "three nodes");
        if let Some(read_only) = second {
            let removed = queue
                .cursor_mut_find_ptr(read_only)
                .and_then(|mut cursor| cursor.remove_current());
            if let Some(node) = removed {
                node.key = 20;
            }
        }
        let first = queue.front().map(NonNull::from);
        assert!(first.is_some(), "two nodes");
        if let Some(read_only) = first {
            let pushed = queue
                .cursor_find_ptr(read_only)
                .and_then(|cursor| cursor.current_ptr());
            if let Some(writable) = pushed {
                // SAFETY: the node was pushed as `&mut`, and no `&Node` is
                // alive.
                unsafe { (*writable.as_ptr()).key = 10 };
            }
        }
    }
    assert_eq!(nodes.map(|node| node.key), [10, 20, 3]);
}

/// Mutants M14 and M15: `IterMut`'s `fold` and `rfold` lend the payloads
/// in order, which a sum cannot tell.
#[test]
fn payload_folds_keep_their_order() {
    let mut nodes = nodes::<4>();
    let mut queue: Queue<'_> = nodes.iter_mut().collect();
    let push = |mut seen: Vec<u32>, key: &mut u32| {
        seen.push(*key);
        seen
    };
    let forward = queue.iter_mut().fold(Vec::new(), push);
    let backward = queue.iter_mut().rfold(Vec::new(), push);
    assert_eq!((forward, backward), (vec![1, 2, 3, 4], vec![4, 3, 2, 1]));
    let mut partly = queue.iter_mut();
    assert!(partly.next().is_some() && partly.next_back().is_some());
    let middle = partly.fold(Vec::new(), push);
    assert_eq!(middle, [2, 3]);
}

/// Mutant M16: an uncounted queue's iterators count every node left, not
/// the lower bound of their size hint, which is two from two nodes on.
#[test]
fn uncounted_iterators_count_every_node() {
    let mut nodes = nodes::<4>();
    let queue: Queue<'_> = nodes.iter_mut().collect();
    let mut iter = queue.iter();
    assert_eq!(iter.clone().count(), 4);
    assert!(iter.next().is_some());
    assert_eq!(iter.count(), 3);
}

/// A node whose payload ends where its link starts.
#[repr(C)]
struct Before {
    key: u64,
    link: Link,
}

/// A node whose payload starts where its link ends.
#[repr(C)]
struct After {
    link: Link,
    key: u64,
}

tailq::adapter!(BeforeLink = Before { link, mut key: u64 });
tailq::adapter!(AfterLink = After { link, mut key: u64 });

/// Mutant M26: a payload may touch the link on either side; only an
/// overlap fails the check.
#[test]
fn payloads_may_touch_the_link() {
    check_payload::<BeforeLink>();
    check_payload::<AfterLink>();
    let mut before = Before {
        key: 1,
        link: Link::new(),
    };
    let mut after = After {
        link: Link::new(),
        key: 2,
    };
    let mut one = TailQueue::<'_, BeforeLink>::new();
    let mut two = TailQueue::<'_, AfterLink>::new();
    one.push_back(&mut before);
    two.push_back(&mut after);
    if let (Some(first), Some(second)) = (one.front_mut(), two.front_mut()) {
        *first = first.wrapping_add(10);
        *second = second.wrapping_add(20);
    }
    let keys = (
        one.front().map(|node| node.key),
        two.back().map(|node| node.key),
    );
    assert_eq!(keys, (Some(11), Some(22)));
}

/// Requirement audit, e54b390: the iterators are `FusedIterator`, which
/// generic code may rely on.  Mutant M33 drops one impl.
#[test]
fn iterators_are_fused() {
    use core::iter::FusedIterator;

    const fn fused<I>(_: &I)
    where
        I: FusedIterator,
    {
    }
    let mut nodes = nodes::<1>();
    let mut queue: Queue<'_> = nodes.iter_mut().collect();
    fused(&queue.iter());
    fused(&queue.iter_ptr());
    fused(&queue.iter_mut());
}

/// Requirement audit, 6a21a14: a head is `repr(C)`, the first node's link,
/// the last node's, then the length.  Mutant M35 drops the `repr(C)`; only
/// a layout that reorders the fields, as `-Zrandomize-layout` may, shows it.
#[test]
fn a_head_holds_first_last_then_count() {
    /// The head's documented layout.
    #[repr(C)]
    struct Words {
        first: Option<NonNull<Link>>,
        last: Option<NonNull<Link>>,
        count: usize,
    }

    let mut nodes = nodes::<3>();
    let ends = (
        nodes.first().map(|node| (&raw const node.link).addr()),
        nodes.last().map(|node| (&raw const node.link).addr()),
    );
    let queue: CountedQueue<'_> = nodes.iter_mut().collect();
    // SAFETY: the head is `repr(C)` with those three words.
    let words = unsafe { (&raw const queue).cast::<Words>().read() };
    let read = (
        words.first.map(NonNull::addr).map(usize::from),
        words.last.map(NonNull::addr).map(usize::from),
    );
    assert_eq!((read, words.count), (ends, 3));
}

/// Requirement audit, 6a21a14: a head in a `ManuallyDrop` leaves its nodes
/// alone when it goes, still marked as linked.
#[test]
fn a_forgotten_head_leaves_its_nodes_marked() {
    use core::mem::ManuallyDrop;

    let mut nodes = nodes::<2>();
    {
        let queue: ManuallyDrop<Queue<'_>> =
            ManuallyDrop::new(nodes.iter_mut().collect());
        assert_eq!(queue.iter().count(), 2);
    }
    assert!(nodes.iter().all(|node| node.link.is_linked()));
}

/// The fourth review found that a lifetime parameter named `'node` met the
/// macro's own binder of that name, and that a type named `Headers` in the
/// bounds met the macro's check item: both are named so that no caller's
/// name meets them now.
#[test]
fn adapters_take_any_names() {
    trait Marked<Mark> {}
    struct Headers;
    impl Marked<Headers> for u8 {}

    struct Named<'node, T> {
        link: Link,
        name: &'node str,
        extra: T,
    }

    tailq::adapter!(
        NamedLink<'node, T> = Named<'node, T> { link, mut name: &'node str }
            where T: Marked<Headers>
    );

    let text = String::from("old");
    let mut node = Named {
        link: Link::new(),
        name: text.as_str(),
        extra: 7,
    };
    let mut queue = TailQueue::<'_, NamedLink<'_, u8>>::new();
    queue.push_back(&mut node);
    if let Some(name) = queue.front_mut() {
        *name = "new";
    }
    assert_eq!(
        queue.front().map(|named| (named.name, named.extra)),
        Some(("new", 7))
    );
}

/// The fourth review found that the third round's promise, that a walk
/// ends early when a node at its other end leaves, did not hold, and the
/// walk contract is now what holds: removing the node a walk has just
/// handed out, from either end.  Here a counted walk alternates ends,
/// removes each node it hands out, and still knows how many it has left.
#[test]
fn a_walk_by_pointer_may_mix_ends_while_removing_what_it_hands_out() {
    let mut nodes = nodes::<5>();
    let base = nodes.as_mut_ptr();
    let mut head = CountedQueue::new();
    let queue = ptr::from_mut(&mut head);
    let mut seen = Vec::new();
    // SAFETY: the nodes outlive the queue and are on no queue; each is
    // removed right after the walk hands it out, and no reference to the
    // queue is alive meanwhile.
    unsafe {
        for i in 0..5 {
            (*queue).push_back_ptr(NonNull::new_unchecked(base.add(i)));
        }
        let mut walk = (*queue).iter_ptr();
        let mut from_back = false;
        while let Some(node) = if from_back {
            walk.next_back()
        } else {
            walk.next()
        } {
            seen.push(((*node.as_ptr()).key, walk.len()));
            (*queue).remove_ptr(node);
            from_back = !from_back;
        }
        assert!((*queue).is_empty() && walk.next().is_none());
    }
    assert_eq!(seen, [(1, 4), (5, 3), (2, 2), (4, 1), (3, 0)]);
}

// Review 4, mutation testing and requirement audit: each test below fails
// for a mutant no other test noticed.  Append to tests/regressions.rs.

/// A node whose payload lies four bytes past a multiple of the link's
/// alignment.
#[repr(C)]
struct Tagged {
    link: Link,
    tag: u32,
    key: u32,
}

tailq::adapter!(TaggedLink = Tagged { link, mut key: u32 });

/// Mutant L02: a payload needs its own alignment, not the link's.
#[test]
fn a_payload_needs_only_its_own_alignment() {
    assert_eq!(payload_offset::<Tagged, u32>(20), 20);
    let mut node = Tagged {
        link: Link::new(),
        tag: 1,
        key: 2,
    };
    let mut queue = TailQueue::<'_, TaggedLink>::new();
    queue.push_back(&mut node);
    if let Some(key) = queue.front_mut() {
        *key = key.wrapping_add(10);
    }
    assert_eq!(
        queue.front().map(|front| (front.tag, front.key)),
        Some((1, 12))
    );
}

/// Mutants R01 and X01, and round 3's M35, which only a reordered layout
/// showed: the head is `repr(C)`, `Counted` `repr(transparent)` and a link
/// `repr(C)`, as the docs promise.  Only then may an `extern "C"` function
/// take them by value: otherwise `improper_ctypes_definitions` fails this
/// file.
#[deny(improper_ctypes_definitions)]
extern "C" fn by_value(queue: CountedQueue<'static>, link: Link) -> usize {
    queue.len().wrapping_add(usize::from(link.is_linked()))
}

#[test]
fn heads_counts_and_links_have_c_layouts() {
    assert_eq!(by_value(CountedQueue::new(), Link::new()), 0);
}

/// Rounds 3 and 4: a walk over a head reached by pointer met a queue that
/// had lost a node the walk had not handed out yet, the node at its other
/// end, and stepped past the queue's end, handed out the node that left, or
/// read its `prev` through `unwrap_unchecked`.  The walk contract forbids
/// that change, and debug builds catch it where the walk meets it, from
/// either end, through `next`, `next_back`, `fold` and `rfold`: as a node
/// that left, when it is the next one the walk hands out, or as a queue
/// that changed, when the walk finds the queue's end before its own.
#[cfg(debug_assertions)]
#[test]
fn a_walk_catches_a_node_it_had_not_handed_out_leaving() {
    use core::panic::AssertUnwindSafe;
    use std::panic::catch_unwind;

    let left = "tail queue: a node the walk had not handed out left the queue";
    let changed = "tail queue: the queue changed under the walk";
    let ways: [u8; 4] = [0, 1, 2, 3];
    for (count, expected) in [(2, left), (4, changed)] {
        for way in ways {
            let backward = !way.is_multiple_of(2);
            let mut nodes = nodes::<4>();
            let base = nodes.as_mut_ptr();
            let mut head = Queue::new();
            let queue = ptr::from_mut(&mut head).cast::<Queue<'static>>();
            let caught = catch_unwind(AssertUnwindSafe(|| {
                // SAFETY: the nodes outlive the queue and are on no queue;
                // the walk removes the node at its other end, which it has
                // not handed out, and no reference to the queue is alive
                // meanwhile.
                unsafe {
                    for i in 0..count {
                        (*queue)
                            .push_back_ptr(NonNull::new_unchecked(base.add(i)));
                    }
                    let far = if backward { 0 } else { count.wrapping_sub(1) };
                    let other = NonNull::new_unchecked(base.add(far));
                    let mut first = true;
                    let mut visit = |_: NonNull<Node>| {
                        if first {
                            first = false;
                            (*queue).remove_ptr(other);
                        }
                    };
                    match way {
                        0 => {
                            for node in (*queue).iter_ptr() {
                                visit(node);
                            }
                        }
                        1 => {
                            for node in (*queue).iter_ptr().rev() {
                                visit(node);
                            }
                        }
                        2 => (*queue).iter_ptr().for_each(&mut visit),
                        _ => (*queue).iter_ptr().rev().for_each(&mut visit),
                    }
                }
            }));
            let message = caught
                .err()
                .and_then(|payload| payload.downcast_ref::<&str>().copied());
            assert_eq!(message, Some(expected), "{count} nodes, way {way}");
            head.clear();
        }
    }
}
