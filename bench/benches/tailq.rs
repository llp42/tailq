// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Times the tail queue on the workloads that tell intrusive shapes apart.
//!
//! Every workload runs whole inside one timed call, over nodes allocated
//! before timing starts, and returns a checksum of the order it saw so
//! the optimizer cannot drop the work.  Ids read `<workload>/<nodes>`.
//! Each fills a queue once, which costs a push per node and, as the queue
//! drops, a store per node; the steady-state workloads then run 16
//! operations per node, so that the fill and the drop weigh little.
//!
//! - `fifo`: push every node at the back, then pop them all from the
//!   front.
//! - `round_robin`: a scheduler's loop, pop the front and push it back.
//! - `deque`: pop at one end and push at the other, alternating ends.
//! - `lru`: touch a node by address, removing it and pushing it at the
//!   front, picked at random, so that most removals are from the middle.
//! - `walk` and `walk_rev`: walk the queue eight times, each way.
//! - `walk_shuffled`: the same forward, over nodes linked in a shuffled
//!   order, so that each step lands on a cold line once the queue
//!   outgrows the cache.
//! - `concat`: deal the nodes round-robin over 16 queues, then append
//!   them all onto the first.

use core::hint::black_box;
use core::ptr::NonNull;
use core::time::Duration;
use criterion::measurement::WallTime;
use criterion::{
    BenchmarkGroup, BenchmarkId, Criterion, criterion_group, criterion_main,
};
use tailq::{Link, TailQueue};

const CONCAT_WAYS: usize = 16;
const STEADY_ROUNDS: usize = 16;
/// Half of `STEADY_ROUNDS`: each round of `deque` runs two operations.
const DEQUE_ROUNDS: usize = 8;
const WALK_ROUNDS: usize = 8;
/// Odd, so that `i * STRIDE mod n` visits every index of a power-of-two
/// `n` once.
const STRIDE: usize = 7919;
const SIZES: [usize; 3] = [1 << 4, 1 << 10, 1 << 16];

const fn mix(hash: u64, key: u64) -> u64 {
    hash.wrapping_mul(31).wrapping_add(key)
}

/// Returns the next of a `SplitMix64` sequence of indices below `n`.
fn random_index(state: &mut u64, n: usize) -> usize {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut mixed = *state;
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let bound = u64::try_from(n).unwrap_or(u64::MAX);
    let index = (mixed ^ (mixed >> 31)).checked_rem(bound).unwrap_or(0);
    usize::try_from(index).unwrap_or(0)
}

/// The `i`th index of a permutation of `0..n`, for a power-of-two `n`.
const fn permuted(i: usize, n: usize) -> usize {
    match i.wrapping_mul(STRIDE).checked_rem(n) {
        Some(index) => index,
        None => 0,
    }
}

/// A node on a tail queue.
struct Node {
    key: u64,
    link: Link,
}

tailq::adapter!(NodeLink = Node { link });

/// A queue of nodes.
type Queue<'nodes> = TailQueue<'nodes, NodeLink>;

const fn node(key: u64) -> Node {
    Node {
        key,
        link: Link::new(),
    }
}

fn fifo(nodes: &mut [Node]) -> u64 {
    let mut queue: Queue<'_> = nodes.iter_mut().collect();
    let mut hash = 0;
    while let Some(node) = queue.pop_front() {
        hash = mix(hash, node.key);
    }
    hash
}

fn round_robin(nodes: &mut [Node]) -> u64 {
    let rounds = STEADY_ROUNDS.wrapping_mul(nodes.len());
    let mut queue: Queue<'_> = nodes.iter_mut().collect();
    let mut hash = 0;
    for _ in 0..rounds {
        if let Some(node) = queue.pop_front() {
            hash = mix(hash, node.key);
            queue.push_back(node);
        }
    }
    hash
}

fn deque(nodes: &mut [Node]) -> u64 {
    let rounds = DEQUE_ROUNDS.wrapping_mul(nodes.len());
    let mut queue: Queue<'_> = nodes.iter_mut().collect();
    let mut hash = 0;
    for _ in 0..rounds {
        if let Some(node) = queue.pop_front() {
            hash = mix(hash, node.key);
            queue.push_back(node);
        }
        if let Some(node) = queue.pop_back() {
            hash = mix(hash, node.key);
            queue.push_front(node);
        }
    }
    hash
}

fn lru(nodes: &mut [Node]) -> u64 {
    let n = nodes.len();
    let base = nodes.as_mut_ptr();
    let mut queue = Queue::new();
    for i in 0..n {
        // SAFETY: the index is in bounds; the nodes outlive the queue,
        // nothing else reaches them, and they are on no queue.
        unsafe { queue.push_back_ptr(NonNull::new_unchecked(base.add(i))) };
    }
    let mut state = 0;
    for _ in 0..STEADY_ROUNDS.wrapping_mul(n) {
        // SAFETY: the index is in bounds, the node is on this queue, and
        // it rejoins the queue once it has left.
        unsafe {
            let node =
                NonNull::new_unchecked(base.add(random_index(&mut state, n)));
            queue.remove_ptr(node);
            queue.push_front_ptr(node);
        }
    }
    queue.iter().fold(0, |acc, node| mix(acc, node.key))
}

fn walk(nodes: &mut [Node]) -> u64 {
    let queue: Queue<'_> = nodes.iter_mut().collect();
    let mut hash = 0;
    for _ in 0..WALK_ROUNDS {
        hash = queue.iter().fold(hash, |acc, node| mix(acc, node.key));
    }
    hash
}

fn walk_rev(nodes: &mut [Node]) -> u64 {
    let queue: Queue<'_> = nodes.iter_mut().collect();
    let mut hash = 0;
    for _ in 0..WALK_ROUNDS {
        hash = queue
            .iter()
            .rev()
            .fold(hash, |acc, node| mix(acc, node.key));
    }
    hash
}

fn walk_shuffled(nodes: &mut [Node]) -> u64 {
    let n = nodes.len();
    let base = nodes.as_mut_ptr();
    let mut queue = Queue::new();
    for i in 0..n {
        // SAFETY: the index is in bounds, and each node is pushed once:
        // the nodes outlive the queue, nothing else reaches them, and
        // they are on no queue.
        unsafe {
            queue.push_back_ptr(NonNull::new_unchecked(
                base.add(permuted(i, n)),
            ));
        }
    }
    let mut hash = 0;
    for _ in 0..WALK_ROUNDS {
        hash = queue.iter().fold(hash, |acc, node| mix(acc, node.key));
    }
    hash
}

fn concat(nodes: &mut [Node]) -> u64 {
    let mut heads = [const { Queue::new() }; CONCAT_WAYS];
    for round in nodes.chunks_mut(CONCAT_WAYS) {
        for (node, lane) in round.iter_mut().zip(&mut heads) {
            lane.push_back(node);
        }
    }
    let Some((first, rest)) = heads.split_first_mut() else {
        return 0;
    };
    for other in rest {
        first.append(other);
    }
    first.iter().fold(0, |acc, node| mix(acc, node.key))
}

/// Times `run` at every size in the group `name`, each call over fresh
/// nodes.
fn bench(criterion: &mut Criterion, name: &str, run: fn(&mut [Node]) -> u64) {
    let mut group = criterion.benchmark_group(name);
    for n in SIZES {
        let mut nodes: Vec<Node> = (0..).take(n).map(node).collect();
        let _: &mut BenchmarkGroup<'_, WallTime> = group.bench_with_input(
            BenchmarkId::from_parameter(n),
            &n,
            |bencher, _| bencher.iter(|| run(black_box(&mut nodes))),
        );
    }
    group.finish();
}

fn tail_queue(criterion: &mut Criterion) {
    bench(criterion, "fifo", fifo);
    bench(criterion, "round_robin", round_robin);
    bench(criterion, "deque", deque);
    bench(criterion, "lru", lru);
    bench(criterion, "walk", walk);
    bench(criterion, "walk_rev", walk_rev);
    bench(criterion, "walk_shuffled", walk_shuffled);
    bench(criterion, "concat", concat);
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2))
        .sample_size(20);
    targets = tail_queue
}
criterion_main!(benches);
