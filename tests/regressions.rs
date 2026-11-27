// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! One test per guarantee, named after what it pins, so that none of them
//! breaks unnoticed: heads that move once emptied, pointers handed back
//! with the access they were pushed with, iterator lengths and folds,
//! walks by pointer that remove or move what they hand out, lookups by
//! address, the layouts of heads and links, the names and paths `adapter!`
//! takes and the checks it runs, how heads, walks and cursors print, and
//! how long walks take.  Run under Miri as well: several of them fail only
//! there.  The misuse that debug builds catch is in `misuse.rs`, and what
//! must not compile is in `src/compile_fail.rs`.

use core::array;
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

/// Once its last node leaves, a head holds no pointer to itself, so an
/// emptied head moves like a new one and takes nodes again in its new
/// place, where both walks find them.
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

/// A cursor built from a pointer reaches the node through the pointer it
/// was pushed with.  Here the pointer comes from a `&Node`, which may not
/// write the node, so Miri fails this test if the cursor kept that
/// pointer.
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

/// `IterMut::last` lends the back payload, also after the back end moved,
/// and nothing once exhausted.
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

/// Debug builds fill a leaving link's forward word with an address no link
/// can have, `usize::MAX`, so that a stale walk faults on a recognisable
/// address.
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

/// A counted queue's iterators know how many nodes they have left, from
/// either end, so their size hint is exact and a `collect` reserves just
/// what it needs.
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

/// `adapter!` takes lifetime and const parameters, and a node type that
/// names a lifetime.
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

/// Safe code can tell whether a queue holds a node named by address, find
/// a cursor on it, and remove it only if the queue holds it.
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

/// The explicit `Clone` of a cursor, `IterMut::fold`, and the `Debug` of a
/// mutable cursor and of a payload iterator run in a test, as complete
/// coverage needs, and answer as they should.
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

/// The checks `adapter!` runs at compile time answer at run time too, both
/// ways, so that coverage reaches them.
#[test]
fn the_compile_time_checks_answer_at_run_time() {
    assert!(aligned::<Node, Link>(0));
    assert!(!aligned::<Node, Link>(1));
    assert!(!aligned::<u8, Link>(0));
    assert_eq!(link_offset::<Node>(0, 0), 0);
    assert_eq!(payload_offset::<Node, u32>(16, 16), 16);
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
    assert_eq!(link_offset::<u8>(0, 0), 0);
}

#[test]
#[should_panic(
    expected = "tailq::adapter!: the payload is not aligned in every node"
)]
fn an_unaligned_payload_fails_its_check() {
    assert_eq!(payload_offset::<Node, u32>(1, 1), 1);
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

/// A walk over a head reached by pointer, as a `static mut` one is, may
/// remove the node it has just handed out, from either end, through `next`
/// and through `fold`, which `for_each` runs: no step reads the link of a
/// node the walk has handed out, which debug builds poison as it leaves.
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

/// `Counted` is one word, and a counted head three, as the head's `repr(C)`
/// layout promises.
#[test]
fn a_count_is_one_word() {
    assert_eq!(size_of::<Counted>(), size_of::<usize>());
    assert_eq!(size_of::<CountedQueue<'static>>(), 3 * size_of::<usize>());
}

/// The membership walk debug builds run for a node named by address goes
/// toward both ends at once, so a node next to either end of a long queue
/// costs a step or two.  A walk toward one end only costs thousands of
/// times more at the other.
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

/// A cursor found by address reaches the node through the pointer it was
/// pushed with, not the one it was found by.  Here that one comes from a
/// `&Node`, which may not write the node, so Miri fails this test if the
/// cursor kept it.
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

/// `IterMut`'s `fold` and `rfold` lend the payloads in order, which a sum
/// cannot tell.
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

/// An uncounted queue's iterators count every node left, not the lower
/// bound of their size hint, which is two from two nodes on.
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

/// A payload may touch the link on either side; only an overlap fails the
/// check.
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

/// Each of the three iterators is `FusedIterator`, which generic code may
/// rely on.
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

/// A head is `repr(C)`: the first node's link, the last node's, then the
/// length.  A head without its `repr(C)` fails here only in a layout that
/// reorders the fields, as `-Zrandomize-layout` may.
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

/// A head in a `ManuallyDrop` leaves its nodes alone when it goes, still
/// marked as linked.
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

/// `adapter!` takes a lifetime parameter named `'node` and a type named
/// `Headers` in its bounds: the macro's own binder and check item are
/// named with the prefix `__tailq`, which the macro reserves.
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

/// The walk contract allows removing the node a walk has just handed out,
/// from either end.  Here a counted walk alternates ends, removes each
/// node it hands out, and still knows how many it has left.
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

/// A node whose payload lies four bytes past a multiple of the link's
/// alignment.
#[repr(C)]
struct Tagged {
    link: Link,
    tag: u32,
    key: u32,
}

tailq::adapter!(TaggedLink = Tagged { link, mut key: u32 });

/// A payload needs its own alignment, not the link's.
#[test]
fn a_payload_needs_only_its_own_alignment() {
    assert_eq!(payload_offset::<Tagged, u32>(20, 20), 20);
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

/// The head is `repr(C)`, `Counted` `repr(transparent)` and a link
/// `repr(C)`, as the docs promise.  Only then may an `extern "C"` function
/// take them by value: otherwise `improper_ctypes_definitions` fails this
/// file, in any layout, where reading a head's words shows a missing
/// `repr(C)` only in a reordered one.
#[deny(improper_ctypes_definitions)]
extern "C" fn by_value(queue: CountedQueue<'static>, link: Link) -> usize {
    queue.len().wrapping_add(usize::from(link.is_linked()))
}

#[test]
fn heads_counts_and_links_have_c_layouts() {
    assert_eq!(by_value(CountedQueue::new(), Link::new()), 0);
}

/// A walk over a head reached by pointer may meet a queue that has lost a
/// node the walk has not handed out yet, the node at its other end.
/// Unchecked, the walk would step past the queue's end, hand out the node
/// that left, or read its `prev` through `unwrap_unchecked`.  The walk
/// contract forbids that change, and debug builds catch it where the walk
/// meets it, from either end, through `next`, `next_back`, `fold` and
/// `rfold`: as a node that left, when it is the next one the walk hands
/// out, or as a queue that changed, when the walk finds the queue's end
/// before its own.
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

/// With nightly's enum paths in `offset_of!` and a const `DerefMut`, the
/// raw place of a link reached through a deref could name another type's
/// link while `offset_of!` measures the node: the place must lie where
/// `offset_of!` says, and the check fails when it does not.
#[test]
#[should_panic(
    expected = "tailq::adapter!: the path reaches the link through a deref"
)]
fn a_link_reached_through_a_deref_fails_its_check() {
    assert_eq!(link_offset::<Node>(0, 8), 0);
}

#[test]
#[should_panic(
    expected = "tailq::adapter!: the path reaches the payload through a deref"
)]
fn a_payload_reached_through_a_deref_fails_its_check() {
    assert_eq!(payload_offset::<Node, u32>(16, -8), 16);
}

/// As above, for a link whose place lies before the node.
#[test]
#[should_panic(
    expected = "tailq::adapter!: the path reaches the link through a deref"
)]
fn a_link_placed_before_its_node_fails_its_check() {
    assert_eq!(link_offset::<Node>(0, -8), 0);
}

/// As above, for a payload whose place lies past where `offset_of!` says.
#[test]
#[should_panic(
    expected = "tailq::adapter!: the path reaches the payload through a deref"
)]
fn a_payload_placed_past_its_offset_fails_its_check() {
    assert_eq!(payload_offset::<Node, u32>(16, 24), 16);
}

/// A walk over a head reached by pointer may move the node it has just
/// handed out to another queue, which is what BSD's `TAILQ_FOREACH_SAFE`
/// is for, from either end, through `fold` and `rfold` as well as `next`
/// and `next_back`.  A removal alone cannot show a step that reads the
/// link after it hands the node out: release builds leave a leaving link's
/// forward word as it was.  A push onto another queue rewrites it.
#[test]
fn a_walk_by_pointer_may_move_what_it_hands_out_to_another_queue() {
    let ways: [u8; 4] = [0, 1, 2, 3];
    for way in ways {
        let mut nodes = nodes::<5>();
        let base = nodes.as_mut_ptr();
        let mut head = Queue::new();
        let mut other = Queue::new();
        let mut seen = Vec::new();
        // SAFETY: the nodes outlive both queues and are on no queue when
        // pushed; the walk moves each node it hands out to `other`, which
        // is not the queue it walks, right after it hands it out, and no
        // reference to either queue is alive meanwhile.
        unsafe {
            let queue = by_pointer::<4>(&mut head, base);
            let others = ptr::from_mut(&mut other).cast::<Queue<'static>>();
            (*others).push_back_ptr(NonNull::new_unchecked(base.add(4)));
            let mut move_out = |node: NonNull<Node>| {
                let key = (*node.as_ptr()).key;
                // A walk that read a link after handing its node out would
                // follow it onto `other`, and then around it for ever.
                assert!(key <= 4, "the walk left its queue for another");
                seen.push(key);
                (*queue).remove_ptr(node);
                (*others).push_front_ptr(node);
            };
            match way {
                0 => (*queue).iter_ptr().for_each(&mut move_out),
                1 => (*queue).iter_ptr().rev().for_each(&mut move_out),
                2 => {
                    for node in (*queue).iter_ptr() {
                        move_out(node);
                    }
                }
                _ => {
                    for node in (*queue).iter_ptr().rev() {
                        move_out(node);
                    }
                }
            }
            assert!((*queue).is_empty(), "every node moved");
        }
        let (walked, moved): ([u32; 4], [u32; 5]) = if way.is_multiple_of(2) {
            ([1, 2, 3, 4], [4, 3, 2, 1, 5])
        } else {
            ([4, 3, 2, 1], [1, 2, 3, 4, 5])
        };
        assert_eq!((seen, keys(&other)), (walked.to_vec(), moved.to_vec()));
    }
}

/// An iterator answers `last` from its back end, and a counted one `count`
/// from its count, in O(1), where the default methods walk every node to
/// the end.  A queue of 16384 nodes takes them no longer than one of two,
/// with fifty times the slack.
#[cfg_attr(miri, ignore = "times a walk")]
#[test]
fn last_and_a_counted_count_take_constant_time() {
    use core::hint::black_box;
    use core::iter::repeat_with;
    use core::time::Duration;
    use std::time::Instant;

    const NODES: usize = 1 << 14;
    const CALLS: usize = 1 << 10;

    /// Times `last` and `count` on every iterator of `queue`, best of three.
    fn time(queue: &mut CountedQueue<'_>) -> Duration {
        let trials: u32 = 3;
        let mut best = Duration::MAX;
        for _ in 0..trials {
            let start = Instant::now();
            for _ in 0..CALLS {
                let _: Option<&Node> =
                    black_box(black_box(queue.iter()).last());
                let _: Option<NonNull<Node>> =
                    black_box(black_box(queue.iter_ptr()).last());
                let _: usize = black_box(black_box(queue.iter()).count());
                let _: usize = black_box(black_box(queue.iter_ptr()).count());
                let _: usize = black_box(black_box(queue.iter_mut()).count());
                let _: Option<&mut u32> =
                    black_box(black_box(queue.iter_mut()).last());
            }
            best = best.min(start.elapsed());
        }
        best
    }

    let mut long: Vec<Node> = repeat_with(|| Node {
        link: Link::new(),
        key: 0,
    })
    .take(NODES)
    .collect();
    let mut short = nodes::<2>();
    let mut long_queue: CountedQueue<'_> = long.iter_mut().collect();
    let mut short_queue: CountedQueue<'_> = short.iter_mut().collect();
    let (long_time, short_time) =
        (time(&mut long_queue), time(&mut short_queue));
    let slack = Duration::from_millis(2);
    assert!(
        long_time < short_time * 50 + slack,
        "{NODES} nodes {long_time:?}, two {short_time:?}"
    );
}

/// At sixteen nodes, `rfold` and `find` from the back take about as long
/// as the same walks from the front: `rfold` less than 1.35 times as long
/// as `fold`, and `find` from the back less than 1.2 times as long as from
/// the front.  The bounds lie between the ratios of steps that cost the
/// same both ways, about 1.05 and 0.95, and those of a backward step that
/// costs more, 1.7 and 1.3.  Optimised builds only, best of fifteen
/// trials, each walk timed beside the others.
#[cfg(not(debug_assertions))]
#[cfg_attr(miri, ignore = "times a walk")]
#[test]
fn backward_walks_cost_what_forward_ones_do() {
    use core::hint::black_box;
    use core::time::Duration;
    use std::time::Instant;

    const WALKS: usize = 1 << 14;
    const TRIALS: usize = 15;

    /// Times `WALKS` runs of `walk`.
    fn time<F>(walk: F) -> Duration
    where
        F: Fn() -> u32,
    {
        let start = Instant::now();
        for _ in 0..WALKS {
            let _: u32 = black_box(walk());
        }
        start.elapsed()
    }

    let mut nodes = nodes::<16>();
    let queue: Queue<'_> = nodes.iter_mut().collect();
    let (first, last) = (queue.front_ptr(), queue.back_ptr());
    let none: u32 = 0;
    let fold = || {
        black_box(&queue)
            .iter()
            .fold(none, |keys, node| keys ^ node.key)
    };
    let rfold = || {
        black_box(&queue)
            .iter()
            .rfold(none, |keys, node| keys ^ node.key)
    };
    let find = || {
        let found = black_box(&queue)
            .iter_ptr()
            .find(|node| Some(*node) == black_box(last));
        u32::from(found.is_some())
    };
    let find_back = || {
        let found = black_box(&queue)
            .iter_ptr()
            .rev()
            .find(|node| Some(*node) == black_box(first));
        u32::from(found.is_some())
    };
    let mut best = [Duration::MAX; 4];
    for _ in 0..TRIALS {
        let took = [time(fold), time(rfold), time(find), time(find_back)];
        for (fastest, this) in best.iter_mut().zip(took) {
            *fastest = (*fastest).min(this);
        }
    }
    let [fold_time, rfold_time, find_time, find_back_time] = best;
    assert!(
        rfold_time * 20 < fold_time * 27 && find_back_time * 5 < find_time * 6,
        "fold {fold_time:?}, rfold {rfold_time:?}, find {find_time:?}, \
         find from the back {find_back_time:?}"
    );
}

/// `adapter!` compiles beside a constant named `node` and a unit struct
/// named `slot`, which a `let` or a closure's parameter cannot shadow: the
/// expansion's locals are named with the prefix `__tailq`, which the macro
/// reserves, as its binder and check item are in `adapters_take_any_names`.
#[test]
#[expect(
    non_upper_case_globals,
    non_camel_case_types,
    reason = "the names the expansion's locals had"
)]
fn adapters_meet_no_constant_or_unit_struct_of_the_callers() {
    const node: u32 = 7;
    struct slot;

    struct Keyed {
        link: Link,
        key: u32,
    }

    tailq::adapter!(KeyedLink = Keyed { link, mut key: u32 });

    let mut one = Keyed {
        link: Link::new(),
        key: node,
    };
    let mut queue = TailQueue::<'_, KeyedLink>::new();
    queue.push_back(&mut one);
    if let Some(key) = queue.front_mut() {
        *key = key.wrapping_add(1);
    }
    assert_eq!(size_of::<slot>(), 0);
    assert_eq!(queue.front().map(|front| front.key), Some(8));
}

/// `adapter!` compiles beside a type of the caller's named `usize`, as it
/// does beside a constant named `node` or a unit struct named `slot`: an
/// adapter's offsets are the primitive `usize`, which the expansion names
/// so that no item of the caller's meets it, and they equal the offsets of
/// the link and the payload.
#[test]
#[expect(
    non_camel_case_types,
    reason = "the name of the type the adapter's offsets have"
)]
fn adapters_meet_no_type_named_usize() {
    type usize = u32;

    struct Keyed {
        link: Link,
        key: usize,
    }

    tailq::adapter!(KeyedLink = Keyed { link, mut key: usize });

    assert_eq!(
        (
            <KeyedLink as tailq::Adapter>::OFFSET,
            <KeyedLink as tailq::Payload>::PAYLOAD_OFFSET
        ),
        (
            core::mem::offset_of!(Keyed, link),
            core::mem::offset_of!(Keyed, key)
        )
    );
    let mut one = Keyed {
        link: Link::new(),
        key: 7,
    };
    let mut queue = TailQueue::<'_, KeyedLink>::new();
    queue.push_back(&mut one);
    if let Some(key) = queue.front_mut() {
        *key = key.wrapping_add(1);
    }
    assert_eq!(queue.front().map(|front| front.key), Some(8));
}

/// The same for a const parameter named `slot`.
#[test]
#[expect(
    non_upper_case_globals,
    reason = "the name a local of the expansion had"
)]
fn adapters_meet_no_const_parameter_of_the_callers() {
    struct Slotted<const slot: usize> {
        link: Link,
        keys: [u32; slot],
    }

    tailq::adapter!(
        SlottedLink<const slot: usize> = Slotted<slot> {
            link,
            mut keys: [u32; slot],
        }
    );

    let mut one = Slotted {
        link: Link::new(),
        keys: [1, 2],
    };
    let mut queue = TailQueue::<'_, SlottedLink<2>>::new();
    queue.push_back(&mut one);
    if let Some(keys) = queue.front_mut() {
        keys.reverse();
    }
    assert_eq!(queue.front().map(|front| front.keys), Some([2, 1]));
}

/// Before a walk hands a node out it checks, in debug builds, that the
/// node is still on the queue, and `last` checks the walk's back node too:
/// it does not hand that node out once it has left.
#[cfg(debug_assertions)]
#[test]
#[should_panic(
    expected = "tail queue: a node the walk had not handed out left the queue"
)]
fn last_catches_the_back_node_leaving() {
    let mut nodes = nodes::<3>();
    let base = nodes.as_mut_ptr();
    let mut head = Queue::new();
    // SAFETY: the nodes outlive the queue and are on no queue; removing the
    // walk's back node, which it has not handed out, is the misuse under
    // test, and no reference to the queue is alive meanwhile.
    unsafe {
        let queue = by_pointer::<3>(&mut head, base);
        let mut walk = (*queue).iter_ptr();
        assert!(walk.next().is_some(), "three nodes");
        (*queue).remove_ptr(NonNull::new_unchecked(base.add(2)));
        let _: Option<NonNull<Node>> = walk.last();
    }
}

/// A cursor borrows its queue, so code that reaches a head by pointer ends
/// its use of one before it changes the queue, unlike a walk, which allows
/// removing the node it just handed out.  Here a cursor finds each node to
/// remove and the one after it, the removal follows, and a new cursor
/// starts from the one after.
#[test]
fn a_cursor_by_pointer_is_done_before_the_queue_changes() {
    let mut nodes = nodes::<4>();
    let base = nodes.as_mut_ptr();
    let mut head = Queue::new();
    let queue = ptr::from_mut(&mut head);
    let mut removed = Vec::new();
    // SAFETY: the nodes are live and unmoved while linked, each pushed
    // once by a pointer derived from `base`, and every cursor is done with
    // before the queue changes.
    unsafe {
        for i in 0..4 {
            (*queue).push_back_ptr(NonNull::new_unchecked(base.add(i)));
        }
        let mut at = (*queue).front_ptr();
        while let Some(node) = at {
            let mut cursor = (*queue).cursor_from_ptr(node);
            cursor.move_next();
            at = cursor.current_ptr();
            if (*node.as_ptr()).key & 1 == 0 {
                (*queue).remove_ptr(node);
                removed.push((*node.as_ptr()).key);
            }
        }
    }
    assert_eq!((removed, keys(&head)), (vec![2, 4], vec![1, 3]));
}

/// Debug builds read links an operation does not touch, so a node's other
/// queues stay unused while one lends its payload, even when the payload
/// holds their links.  Here the run queue lends a thread's scheduling
/// state, which holds its link on the queue of every thread, and that
/// queue is used only once the loan ends.
#[test]
fn a_payload_holding_another_queues_link_is_lent_while_that_queue_rests() {
    struct Thread {
        run: Link,
        sched: Sched,
    }

    struct Sched {
        all: Link,
        prio: u32,
    }

    tailq::adapter!(RunLink = Thread { run, mut sched: Sched });
    tailq::adapter!(AllLink = Thread { sched.all });

    let mut threads: [Thread; 5] = array::from_fn(|prio| Thread {
        run: Link::new(),
        sched: Sched {
            all: Link::new(),
            prio: u32::try_from(prio).unwrap_or(u32::MAX),
        },
    });
    let base = threads.as_mut_ptr();
    let mut all = TailQueue::<'_, AllLink>::new();
    let mut run = TailQueue::<'_, RunLink>::new();
    // SAFETY: the threads are live and unmoved while linked, each pushed
    // onto each queue by the same pointer derived from `base`; the queue
    // of every thread rests while the run queue lends a payload, and the
    // borrower stays off the link inside it.
    unsafe {
        for i in 0..5 {
            all.push_back_ptr(NonNull::new_unchecked(base.add(i)));
        }
        run.push_back_ptr(NonNull::new_unchecked(base.add(1)));
        if let Some(sched) = run.front_mut() {
            sched.prio = sched.prio.wrapping_add(10);
        }
        all.remove_ptr(NonNull::new_unchecked(base.add(3)));
        if let Some(sched) = run.front_mut() {
            sched.prio = sched.prio.wrapping_add(1);
        }
        let prios: Vec<u32> =
            all.iter().map(|thread| thread.sched.prio).collect();
        assert_eq!(prios, [0, 12, 2, 4]);
        run.clear();
        all.clear();
    }
}

/// Every local of the expansion is named with the prefix `__tailq`, which
/// the macro reserves, not only `node` and `slot`.  A local named `at` would meet a
/// constant, which a `let` cannot shadow, and one named `spare` a unit
/// struct.
#[test]
#[expect(
    non_upper_case_globals,
    non_camel_case_types,
    reason = "the names the expansion's locals would have without their prefix"
)]
fn adapters_meet_no_item_named_like_their_locals() {
    const at: isize = 0;
    struct spare;

    struct Keyed {
        link: Link,
        key: u32,
    }

    tailq::adapter!(SpareLink = Keyed { link, mut key: u32 });

    let mut one = Keyed {
        link: Link::new(),
        key: 7,
    };
    let mut queue = TailQueue::<'_, SpareLink>::new();
    queue.push_back(&mut one);
    if let Some(key) = queue.front_mut() {
        *key = key.wrapping_add(1);
    }
    assert_eq!((at, size_of::<spare>()), (0, 0));
    assert_eq!(queue.front().map(|front| front.key), Some(8));
}

/// A node named by address is reached through the pointer given, which
/// must reach its link: one taken from a `&Node` does, as a link's words
/// are cells, and so does one saved from the push, while one taken from a
/// lent `&mut` payload reaches the payload only.  Here a walk by pointer
/// removes what it hands out through each of the first two.
#[test]
fn a_walk_by_pointer_removes_through_a_pointer_that_reaches_the_link() {
    for backward in [false, true] {
        let mut nodes = nodes::<4>();
        let base = nodes.as_mut_ptr();
        let mut head = Queue::new();
        let queue = ptr::from_mut(&mut head);
        // SAFETY: the nodes are live and unmoved while linked, each pushed
        // once by a pointer derived from `base`, and the walk removes only
        // the node it just handed out, through a pointer taken from the
        // shared node it handed out.
        unsafe {
            for i in 0..4 {
                (*queue).push_back_ptr(NonNull::new_unchecked(base.add(i)));
            }
            let take = |node: &Node| {
                if node.key & 1 == 0 {
                    (*queue).remove_ptr(NonNull::from(node));
                }
            };
            if backward {
                (*queue).iter().rev().for_each(take);
            } else {
                (*queue).iter().for_each(take);
            }
        }
        assert_eq!(keys(&head), [1, 3], "backward: {backward}");
    }
    for backward in [false, true] {
        let mut nodes = nodes::<4>();
        let base = nodes.as_mut_ptr();
        let mut head = Queue::new();
        let queue = ptr::from_mut(&mut head);
        // SAFETY: as above, through the pointers saved from the pushes,
        // while the payload the walk lent is no longer used.
        unsafe {
            let saved: Vec<NonNull<Node>> = (0..4)
                .map(|i| NonNull::new_unchecked(base.add(i)))
                .collect();
            for &node in &saved {
                (*queue).push_back_ptr(node);
            }
            let take = |key: &mut u32| {
                let odd = *key & 1 == 1;
                let at = usize::try_from(*key).unwrap_or(0).wrapping_sub(1);
                *key = key.wrapping_add(10);
                if let Some(&node) = saved.get(at).filter(|_| odd) {
                    (*queue).remove_ptr(node);
                }
            };
            if backward {
                (*queue).iter_mut().rev().for_each(take);
            } else {
                (*queue).iter_mut().for_each(take);
            }
        }
        assert_eq!(keys(&head), [12, 14], "backward: {backward}");
    }
}

/// A head, a walk and a cursor print the links they hold, each under its
/// own name: a head its first and last, a walk its front and back, a
/// cursor the link it rests on.  Each name is checked with its link's
/// address, which the type names and the head's count do not show.
#[test]
fn heads_walks_and_cursors_print_their_own_links() {
    let mut nodes = nodes::<3>();
    // Only printed, never read through.
    let [first, middle, last] =
        nodes.each_ref().map(|node| Some(NonNull::from(&node.link)));
    let queue: Queue<'_> = nodes.iter_mut().collect();
    let has = |shown: &str, fields: &[(&str, Option<NonNull<Link>>)]| {
        fields
            .iter()
            .all(|&(name, link)| shown.contains(&format!("{name}: {link:?}")))
    };
    let head = format!("{queue:?}");
    assert!(has(&head, &[("first", first), ("last", last)]), "{head}");
    let mut walk = queue.iter_ptr();
    let ends = format!("{walk:?}");
    assert!(has(&ends, &[("front", first), ("back", last)]), "{ends}");
    assert!(walk.next().is_some() && walk.next_back().is_some());
    let met = format!("{walk:?}");
    assert!(has(&met, &[("front", middle), ("back", middle)]), "{met}");
    let mut by_ref = queue.iter();
    assert!(by_ref.next_back().is_some());
    let stepped = format!("{by_ref:?}");
    assert!(
        has(&stepped, &[("front", first), ("back", middle)]),
        "{stepped}"
    );
    let rest = format!("{:?}", queue.cursor_back());
    assert!(has(&rest, &[("current", last)]), "{rest}");
}

/// A task owned by pointer, which leaves its queue when it is dropped.
struct Task {
    key: u32,
    link: Link,
    /// The queue the task may be on, reached by pointer.
    queue: *mut TailQueue<'static, TaskLink>,
}

tailq::adapter!(TaskLink = Task { link });

impl Drop for Task {
    fn drop(&mut self) {
        let task = NonNull::from(&mut *self);
        // SAFETY: the queue outlives every task, and `task` comes from the
        // `Box` rebuilt from the pointer the task was pushed with.
        unsafe {
            if (*self.queue).contains_ptr(task) {
                (*self.queue).remove_ptr(task);
            }
        }
    }
}

/// A node can take itself off its queue while it is borrowed as `&mut`,
/// as its `Drop` borrows it: `contains_ptr` and `remove_ptr`, given that
/// node, reach it only through the pointer given, and the lookup's debug
/// check does not read the link it finds.  Here each of three tasks on
/// the heap, first, middle and last, is freed through the `Box` rebuilt
/// from its push pointer while it is linked, and the other two stay in
/// order.
#[test]
fn a_node_borrowed_by_its_drop_looks_itself_up_and_leaves() {
    for at in 0..3 {
        let mut head = TailQueue::<'static, TaskLink>::new();
        let queue = ptr::from_mut(&mut head);
        let tasks: Vec<NonNull<Task>> = (0..3)
            .map(|key| {
                NonNull::from(Box::leak(Box::new(Task {
                    key,
                    link: Link::new(),
                    queue,
                })))
            })
            .collect();
        // SAFETY: each task is live, on no queue when pushed, and reached
        // only through its push pointer until the `Box` rebuilt from it
        // frees it; the queue is reached only through `queue`.
        unsafe {
            for &task in &tasks {
                (*queue).push_back_ptr(task);
            }
            drop(Box::from_raw(tasks[at].as_ptr()));
            let left: Vec<u32> = (*queue).iter().map(|task| task.key).collect();
            let mut keys = vec![0, 1, 2];
            keys.remove(at);
            assert_eq!(left, keys);
            while let Some(task) = (*queue).pop_front_ptr() {
                drop(Box::from_raw(task.as_ptr()));
            }
        }
    }
}
