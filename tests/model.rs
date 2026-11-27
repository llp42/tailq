// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Random operation sequences on queues, checked after every step against
//! a `VecDeque` of keys: both directions, both ends, the iterators meeting
//! in the middle, cursor walks through the ghost, link marks and lengths.
//! Under Miri the budget is small.

extern crate alloc;

use alloc::collections::VecDeque;
use core::fmt::Debug;
use core::mem::replace;
use core::ptr::{self, NonNull};
use tailq::{Adapter, Count, Counted, Link, TailQueue, Uncounted};

/// `SplitMix64`: deterministic, and enough to shuffle operations.
struct Rng(u64);

impl Rng {
    const fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.0;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    /// Returns a number below `n`, or 0 when `n` is 0.
    fn below(&mut self, n: usize) -> usize {
        let bound = u64::try_from(n).unwrap_or(u64::MAX);
        let draw = self.next_u64().checked_rem(bound).unwrap_or(0);
        usize::try_from(draw).unwrap_or(0)
    }

    const fn coin(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

/// Seeds, operations per seed, and nodes.
const fn budget() -> (u64, usize, usize) {
    if cfg!(miri) {
        (2, 120, 10)
    } else {
        (64, 2000, 40)
    }
}

/// The model position after a cursor's `move_next`.
const fn step_next(pos: Option<usize>, len: usize) -> Option<usize> {
    match pos {
        Some(i) if i + 1 < len => Some(i + 1),
        None if len > 0 => Some(0),
        Some(_) | None => None,
    }
}

/// The model position after a cursor's `move_prev`.
const fn step_prev(pos: Option<usize>, len: usize) -> Option<usize> {
    match pos {
        Some(0) => None,
        Some(i) => Some(i - 1),
        None if len > 0 => Some(len - 1),
        None => None,
    }
}

/// The key at a model position.
fn at_pos<K>(model: &VecDeque<K>, pos: Option<usize>) -> Option<K>
where
    K: Copy,
{
    model.get(pos?).copied()
}

/// Checks a queue against its model in every way it can be read.
fn check<A, C, K, F>(
    rng: &mut Rng,
    queue: &TailQueue<'_, A, C>,
    model: &VecDeque<K>,
    key: F,
) where
    A: Adapter,
    C: Count,
    K: Copy + PartialEq + Debug,
    F: Fn(&A::Node) -> K,
{
    let forward: Vec<K> = queue.iter().map(&key).collect();
    assert!(forward.iter().eq(model.iter()), "{forward:?} {model:?}");
    let backward: Vec<K> = queue.iter().rev().map(&key).collect();
    assert!(backward.iter().eq(model.iter().rev()));
    assert_eq!(queue.is_empty(), model.is_empty());
    assert_eq!(queue.front().map(&key), model.front().copied());
    assert_eq!(queue.back().map(&key), model.back().copied());
    assert_eq!(queue.iter().last().map(&key), model.back().copied());

    let refs: Vec<*const A::Node> = queue.iter().map(ptr::from_ref).collect();
    let ptrs: Vec<*const A::Node> = queue
        .iter_ptr()
        .map(|node| node.as_ptr().cast_const())
        .collect();
    assert_eq!(refs, ptrs);
    let mut ptrs_rev: Vec<*const A::Node> = queue
        .iter_ptr()
        .rev()
        .map(|node| node.as_ptr().cast_const())
        .collect();
    ptrs_rev.reverse();
    assert_eq!(refs, ptrs_rev);

    check_meeting(rng, queue, model, &key);
    check_cursors(queue, model, &key);
}

/// Checks that a random mix of `next` and `next_back` meets exactly once.
fn check_meeting<A, C, K, F>(
    rng: &mut Rng,
    queue: &TailQueue<'_, A, C>,
    model: &VecDeque<K>,
    key: F,
) where
    A: Adapter,
    C: Count,
    K: Copy + PartialEq + Debug,
    F: Fn(&A::Node) -> K,
{
    let mut iter = queue.iter();
    let (mut low, mut high) = (0, model.len());
    while low < high {
        let (at_least, at_most) = iter.size_hint();
        assert!(at_least >= 1 && at_least <= high - low);
        assert!(at_most.is_none_or(|most| most >= high - low));
        if rng.coin() {
            assert_eq!(iter.next().map(&key), model.get(low).copied());
            low += 1;
        } else {
            high -= 1;
            assert_eq!(iter.next_back().map(&key), model.get(high).copied());
        }
    }
    assert_eq!(iter.size_hint(), (0, Some(0)));
    assert!(iter.next().is_none() && iter.next_back().is_none());
}

/// Walks cursors forward and back through the ghost, past both ends.
fn check_cursors<A, C, K, F>(
    queue: &TailQueue<'_, A, C>,
    model: &VecDeque<K>,
    key: F,
) where
    A: Adapter,
    C: Count,
    K: Copy + PartialEq + Debug,
    F: Fn(&A::Node) -> K,
{
    let len = model.len();
    let mut cursor = queue.cursor_front();
    let mut pos = (len > 0).then_some(0);
    for _ in 0..len + 2 {
        assert_eq!(cursor.current().map(&key), at_pos(model, pos));
        assert_eq!(
            cursor.peek_next().map(&key),
            at_pos(model, step_next(pos, len))
        );
        assert_eq!(
            cursor.peek_prev().map(&key),
            at_pos(model, step_prev(pos, len))
        );
        cursor.move_next();
        pos = step_next(pos, len);
    }
    let mut back = queue.cursor_back();
    let mut back_pos = len.checked_sub(1);
    for _ in 0..len + 2 {
        assert_eq!(back.current().map(&key), at_pos(model, back_pos));
        back.move_prev();
        back_pos = step_prev(back_pos, len);
    }
}

/// What a node of the safe model carries besides its link.
struct Info {
    key: usize,
    visits: u32,
}

/// A node of the safe model.
struct Node {
    link: Link,
    info: Info,
}

tailq::adapter!(NodeLink = Node { link, mut info: Info });

/// A counted queue of the safe model.
type Queue<'nodes> = TailQueue<'nodes, NodeLink, Counted>;

/// Two queues of nodes lent as `&mut`, their models, and the nodes on
/// neither.
struct Safe<'nodes> {
    queues: [Queue<'nodes>; 2],
    models: [VecDeque<usize>; 2],
    pool: Vec<&'nodes mut Node>,
    visits: Vec<u32>,
    /// Nodes cleared off a queue, which stay borrowed for `'nodes`.
    lost: usize,
}

impl Safe<'_> {
    fn step(&mut self, rng: &mut Rng) {
        let which = rng.below(2);
        let (left, right) = self.queues.split_at_mut(1);
        let (queue, other) = if which == 0 {
            (&mut left[0], &mut right[0])
        } else {
            (&mut right[0], &mut left[0])
        };
        let (left_model, right_model) = self.models.split_at_mut(1);
        let (model, other_model) = if which == 0 {
            (&mut left_model[0], &mut right_model[0])
        } else {
            (&mut right_model[0], &mut left_model[0])
        };
        match rng.below(100) {
            0..=29 if !self.pool.is_empty() => {
                let node = self.pool.swap_remove(rng.below(self.pool.len()));
                assert!(!node.link.is_linked());
                if rng.coin() {
                    model.push_front(node.info.key);
                    queue.push_front(node);
                } else {
                    model.push_back(node.info.key);
                    queue.push_back(node);
                }
            }
            30..=45 => {
                let popped = if rng.coin() {
                    (queue.pop_front(), model.pop_front())
                } else {
                    (queue.pop_back(), model.pop_back())
                };
                assert_eq!(
                    popped.0.as_ref().map(|node| node.info.key),
                    popped.1
                );
                if let Some(node) = popped.0 {
                    assert!(!node.link.is_linked());
                    self.pool.push(node);
                }
            }
            46..=79 => cursor_step(rng, queue, model, &mut self.pool),
            80..=89 => {
                queue.append(other);
                model.append(other_model);
            }
            90 => {
                self.lost += model.len();
                model.clear();
                queue.clear();
            }
            _ => {
                for info in &mut *queue {
                    info.visits += 1;
                }
                for &key in &*model {
                    self.visits[key] += 1;
                }
            }
        }
    }

    fn check(&self, rng: &mut Rng, nodes: usize) {
        for (queue, model) in self.queues.iter().zip(&self.models) {
            assert_eq!(queue.len(), model.len());
            check(rng, queue, model, |node: &Node| node.info.key);
            assert!(queue.iter().all(|node| node.link.is_linked()));
        }
        let held = self.models.iter().map(VecDeque::len).sum::<usize>();
        assert_eq!(self.pool.len() + held + self.lost, nodes);
        for node in &self.pool {
            assert!(!node.link.is_linked());
            assert_eq!(node.info.visits, self.visits[node.info.key]);
        }
    }
}

/// Walks a mutable cursor to a random position, the ghost included,
/// links, unlinks or replaces a node there, and checks it stays coherent.
fn cursor_step<'nodes>(
    rng: &mut Rng,
    queue: &mut Queue<'nodes>,
    model: &mut VecDeque<usize>,
    pool: &mut Vec<&'nodes mut Node>,
) {
    let len = model.len();
    let from_back = rng.coin();
    let mut cursor = if from_back {
        queue.cursor_back_mut()
    } else {
        queue.cursor_front_mut()
    };
    let mut pos = if from_back {
        len.checked_sub(1)
    } else {
        (len > 0).then_some(0)
    };
    for _ in 0..rng.below(len + 3) {
        if rng.coin() {
            cursor.move_next();
            pos = step_next(pos, len);
        } else {
            cursor.move_prev();
            pos = step_prev(pos, len);
        }
        assert_eq!(
            cursor.current().map(|node| node.info.key),
            at_pos(model, pos)
        );
    }
    let spare = (!pool.is_empty()).then(|| rng.below(pool.len()));
    match (rng.below(4), spare) {
        (0, Some(index)) => {
            let node = pool.swap_remove(index);
            model.insert(pos.map_or(0, |i| i + 1), node.info.key);
            cursor.insert_after(node);
        }
        (1, Some(index)) => {
            let node = pool.swap_remove(index);
            if let Some(i) = pos {
                model.insert(i, node.info.key);
                pos = Some(i + 1);
            } else {
                model.push_back(node.info.key);
            }
            cursor.insert_before(node);
        }
        (2, Some(index)) => {
            let node = pool.swap_remove(index);
            let key = node.info.key;
            match cursor.replace_current(node) {
                Ok(old) => {
                    assert!(!old.link.is_linked());
                    assert_eq!(
                        pos.map(|i| replace(&mut model[i], key)),
                        Some(old.info.key)
                    );
                    pool.push(old);
                }
                Err(back) => {
                    assert!(pos.is_none() && !back.link.is_linked());
                    pool.push(back);
                }
            }
        }
        _ => {
            let removed = cursor.remove_current();
            assert_eq!(
                removed.as_ref().map(|node| node.info.key),
                pos.and_then(|i| model.remove(i))
            );
            if let Some(node) = removed {
                assert!(!node.link.is_linked());
                pool.push(node);
                pos = pos.filter(|&i| i < model.len());
            }
        }
    }
    let after = model.len();
    if let Some(info) = cursor.current_mut() {
        assert_eq!(Some(info.key), at_pos(model, pos));
    }
    assert_eq!(
        cursor.peek_next().map(|node| node.info.key),
        at_pos(model, step_next(pos, after))
    );
    assert_eq!(
        cursor.peek_prev().map(|node| node.info.key),
        at_pos(model, step_prev(pos, after))
    );
}

#[test]
fn safe_api_matches_the_model() {
    let (seeds, ops, n) = budget();
    for seed in 0..seeds {
        let mut rng = Rng(seed);
        let mut nodes: Vec<Node> = (0..n)
            .map(|key| Node {
                link: Link::new(),
                info: Info { key, visits: 0 },
            })
            .collect();
        let mut safe = Safe {
            queues: [Queue::new(), Queue::new()],
            models: [VecDeque::new(), VecDeque::new()],
            pool: nodes.iter_mut().collect(),
            visits: vec![0; n],
            lost: 0,
        };
        for _ in 0..ops {
            safe.step(&mut rng);
            safe.check(&mut rng, n);
        }
    }
}

/// A node of the raw model, on up to two queues at once.
struct Shared {
    key: usize,
    hits: u32,
    one: Link,
    two: Link,
}

tailq::adapter!(OneLink = Shared { one, mut hits: u32 });
tailq::adapter!(TwoLink = Shared { two });

/// Two queues through one link, their models, and where each node is.
struct Family<A: Adapter<Node = Shared>, C: Count> {
    queues: [TailQueue<'static, A, C>; 2],
    models: [VecDeque<usize>; 2],
    on: Vec<Option<usize>>,
}

impl<A: Adapter<Node = Shared>, C: Count> Family<A, C> {
    fn new(nodes: usize) -> Self {
        Self {
            queues: [TailQueue::new(), TailQueue::new()],
            models: [VecDeque::new(), VecDeque::new()],
            on: vec![None; nodes],
        }
    }

    /// A node on neither queue of this family, if any.
    fn free(&self, rng: &mut Rng) -> Option<usize> {
        let free: Vec<usize> = (0..self.on.len())
            .filter(|&i| self.on[i].is_none())
            .collect();
        (!free.is_empty()).then(|| free[rng.below(free.len())])
    }

    /// Takes one random step through the `_ptr` operations.
    ///
    /// # Safety
    ///
    /// `at(i)` must point at the live node `i` of a set no other code
    /// reaches meanwhile, and `index` must invert it.
    unsafe fn step<At, Index>(&mut self, rng: &mut Rng, at: At, index: Index)
    where
        At: Fn(usize) -> NonNull<Shared>,
        Index: Fn(NonNull<Shared>) -> usize + Copy,
    {
        let which = rng.below(2);
        let free = self.free(rng);
        let (left, right) = self.queues.split_at_mut(1);
        let (queue, other) = if which == 0 {
            (&mut left[0], &mut right[0])
        } else {
            (&mut right[0], &mut left[0])
        };
        let (left_model, right_model) = self.models.split_at_mut(1);
        let (model, other_model) = if which == 0 {
            (&mut left_model[0], &mut right_model[0])
        } else {
            (&mut right_model[0], &mut left_model[0])
        };
        // SAFETY: every node pushed is on no queue of this family, every
        // node removed or reached by address is on `queue`, and the nodes
        // stay live and reached only through the queues.
        unsafe {
            match (rng.below(10), free) {
                (0, Some(i)) => {
                    queue.push_front_ptr(at(i));
                    model.push_front(i);
                }
                (1, Some(i)) => {
                    queue.push_back_ptr(at(i));
                    model.push_back(i);
                }
                (2, _) => assert_eq!(
                    queue.pop_front_ptr().map(index),
                    model.pop_front()
                ),
                (3, _) => assert_eq!(
                    queue.pop_back_ptr().map(index),
                    model.pop_back()
                ),
                (4, _) if !model.is_empty() => {
                    let i = model
                        .remove(rng.below(model.len()))
                        .unwrap_or(usize::MAX);
                    queue.remove_ptr(at(i));
                }
                (5 | 6, _) if !model.is_empty() => {
                    let j = rng.below(model.len());
                    address_step(rng, queue, model, j, free, &at, index);
                }
                (7, _) => {
                    queue.append(other);
                    model.append(other_model);
                }
                (8, _) if rng.below(4) == 0 => {
                    queue.clear();
                    model.clear();
                }
                _ => {
                    for node in queue.iter_ptr() {
                        (*node.as_ptr()).hits += 1;
                    }
                    if let Some(node) = queue.pop_front() {
                        node.hits += 1;
                        let i = node.key;
                        assert_eq!(model.pop_front(), Some(i));
                        queue.push_back_ptr(at(i));
                        model.push_back(i);
                    }
                }
            }
        }
        self.on.fill(None);
        for (which_queue, keys) in self.models.iter().enumerate() {
            for &i in keys {
                self.on[i] = Some(which_queue);
            }
        }
    }

    fn check(&self, rng: &mut Rng) {
        for (queue, model) in self.queues.iter().zip(&self.models) {
            check(rng, queue, model, |node: &Shared| node.key);
        }
    }
}

/// Reaches the node at model position `j` by address, through a shared
/// cursor and then a mutable one, and links, replaces or unlinks there.
///
/// # Safety
///
/// As for [`Family::step`], with `free` on no queue of the family.
unsafe fn address_step<A, C, At, Index>(
    rng: &mut Rng,
    queue: &mut TailQueue<'static, A, C>,
    model: &mut VecDeque<usize>,
    j: usize,
    free: Option<usize>,
    at: At,
    index: Index,
) where
    A: Adapter<Node = Shared>,
    C: Count,
    At: Fn(usize) -> NonNull<Shared>,
    Index: Fn(NonNull<Shared>) -> usize + Copy,
{
    let len = model.len();
    let i = model[j];
    // SAFETY: the node at `j` is on `queue`, and `free` on no queue of
    // this family.
    unsafe {
        let shared = queue.cursor_from_ptr(at(i));
        assert_eq!(shared.current_ptr().map(index), Some(i));
        assert_eq!(
            shared.peek_next_ptr().map(index),
            at_pos(model, step_next(Some(j), len))
        );
        assert_eq!(
            shared.peek_prev_ptr().map(index),
            at_pos(model, step_prev(Some(j), len))
        );
        let mut cursor = queue.cursor_mut_from_ptr(at(i));
        match (rng.below(4), free) {
            (0, Some(spare)) => {
                cursor.insert_after_ptr(at(spare));
                model.insert(j + 1, spare);
            }
            (1, Some(spare)) => {
                cursor.insert_before_ptr(at(spare));
                model.insert(j, spare);
            }
            (2, Some(spare)) => {
                assert_eq!(
                    cursor.replace_current_ptr(at(spare)).map(index),
                    Some(i)
                );
                model[j] = spare;
                assert_eq!(cursor.current_ptr().map(index), Some(spare));
            }
            _ => {
                assert_eq!(cursor.remove_current_ptr().map(index), Some(i));
                let _: Option<usize> = model.remove(j);
                assert_eq!(
                    cursor.current_ptr().map(index),
                    model.get(j).copied()
                );
            }
        }
    }
}

#[test]
fn ptr_api_with_two_links_matches_the_model() {
    let (seeds, ops, n) = budget();
    for seed in 0..seeds {
        let mut rng = Rng(seed ^ 0xA5A5);
        let boxed: Box<[Shared]> = (0..n)
            .map(|key| Shared {
                key,
                hits: 0,
                one: Link::new(),
                two: Link::new(),
            })
            .collect();
        let base: *mut Shared = Box::into_raw(boxed).cast();
        // SAFETY: every index is in bounds.
        let at = |i: usize| unsafe { NonNull::new_unchecked(base.add(i)) };
        // SAFETY: every pointer is into the same allocation, at or after
        // `base`.
        let index = |node: NonNull<Shared>| unsafe {
            node.as_ptr().offset_from_unsigned(base)
        };
        {
            let mut one = Family::<OneLink, Counted>::new(n);
            let mut two = Family::<TwoLink, Uncounted>::new(n);
            for _ in 0..ops {
                // SAFETY: the nodes stay live until the families drop, and
                // only the queues reach them meanwhile.
                unsafe {
                    if rng.coin() {
                        one.step(&mut rng, at, index);
                    } else {
                        two.step(&mut rng, at, index);
                    }
                }
                one.check(&mut rng);
                two.check(&mut rng);
                for (queue, model) in one.queues.iter().zip(&one.models) {
                    assert_eq!(queue.len(), model.len());
                }
                for i in 0..n {
                    let node = at(i).as_ptr();
                    // SAFETY: the link fields are read only through their
                    // cells, while no queue writes them.
                    unsafe {
                        assert_eq!(
                            (*node).one.is_linked(),
                            one.on[i].is_some()
                        );
                        assert_eq!(
                            (*node).two.is_linked(),
                            two.on[i].is_some()
                        );
                    }
                }
            }
            // The families drop here, and unmark what they hold.
        }
        // SAFETY: the slice came from `Box::into_raw`, and no queue holds
        // a node of it any more.
        let nodes =
            unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(base, n)) };
        assert!(
            nodes
                .iter()
                .all(|node| !node.one.is_linked() && !node.two.is_linked())
        );
    }
}
