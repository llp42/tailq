// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Times the tail queue on the workloads that tell intrusive shapes apart.
//!
//! Every workload runs whole inside one timed call, over nodes allocated
//! before timing starts, and returns a checksum of the order it saw so
//! the optimizer cannot drop the walk.  Ids read `<workload>/<nodes>`.
//!
//! - `fifo`: push every node at the back, then pop them all from the
//!   front.
//! - `walk`: push every node, then walk the queue eight times.
//! - `walk_rev`: the same, back to front.
//! - `churn`: push every node, then 256 times remove a node by address
//!   and push it back at the front.
//! - `concat`: deal the nodes round-robin over 16 queues, then append
//!   them all onto the first.

use core::hint::black_box;
use core::pin::{Pin, pin};
use core::ptr::NonNull;
use core::time::Duration;
use criterion::measurement::WallTime;
use criterion::{
    BenchmarkGroup, BenchmarkId, Criterion, criterion_group, criterion_main,
};
use tailq::Link;

const CHURN_OPS: usize = 256;
const CHURN_STRIDE: usize = 7919;
const CONCAT_WAYS: usize = 16;
const WALK_ROUNDS: usize = 8;
const SIZES: [usize; 3] = [1 << 4, 1 << 10, 1 << 16];

const fn mix(hash: u64, key: u64) -> u64 {
    hash.wrapping_mul(31).wrapping_add(key)
}

const fn churn_index(i: usize, n: usize) -> usize {
    match i.wrapping_mul(CHURN_STRIDE).checked_rem(n) {
        Some(index) => index,
        None => 0,
    }
}

/// A node on a tail queue.
struct Node {
    key: u64,
    link: Link,
}

tailq::queue!(Queue<'nodes> = Node { link });

const fn node(key: u64) -> Node {
    Node {
        key,
        link: Link::new(),
    }
}

/// Returns the `i`th head of a pinned array of heads.
fn lane<Q, const N: usize>(heads: Pin<&mut [Q; N]>, i: usize) -> Pin<&mut Q> {
    // SAFETY: an element of a pinned array is pinned too, and nothing
    // moves it out.
    unsafe { heads.map_unchecked_mut(|all| all.get_unchecked_mut(i)) }
}

/// Returns the first head and the `w`th of a pinned array of heads.
fn first_and<Q, const N: usize>(
    heads: Pin<&mut [Q; N]>,
    w: usize,
) -> (Pin<&mut Q>, Pin<&mut Q>) {
    // SAFETY: elements of a pinned array are pinned too, nothing moves
    // them out, and for `w > 0` the two borrows are disjoint.
    unsafe {
        let (first, rest) = heads.get_unchecked_mut().split_at_mut(w);
        (
            Pin::new_unchecked(first.get_unchecked_mut(0)),
            Pin::new_unchecked(rest.get_unchecked_mut(0)),
        )
    }
}

fn fifo(nodes: &mut [Node]) -> u64 {
    let mut queue = pin!(Queue::new());
    for node in nodes {
        queue.as_mut().push_back(node);
    }
    let mut hash = 0;
    while let Some(node) = queue.as_mut().cursor_front_mut().remove_current() {
        hash = mix(hash, node.key);
    }
    hash
}

fn walk(nodes: &mut [Node]) -> u64 {
    let mut queue = pin!(Queue::new());
    for node in nodes {
        queue.as_mut().push_back(node);
    }
    let mut hash = 0;
    for _ in 0..WALK_ROUNDS {
        hash = queue.iter().fold(hash, |acc, node| mix(acc, node.key));
    }
    hash
}

fn walk_rev(nodes: &mut [Node]) -> u64 {
    let mut queue = pin!(Queue::new());
    for node in nodes {
        queue.as_mut().push_back(node);
    }
    let mut hash = 0;
    for _ in 0..WALK_ROUNDS {
        hash = queue
            .iter()
            .rev()
            .fold(hash, |acc, node| mix(acc, node.key));
    }
    hash
}

fn churn(nodes: &mut [Node]) -> u64 {
    let n = nodes.len();
    let base = nodes.as_mut_ptr();
    let mut queue = pin!(Queue::new());
    for i in 0..n {
        // SAFETY: the index is in bounds; the nodes outlive the queue,
        // nothing else reaches them, and they are on no queue.
        unsafe {
            queue
                .as_mut()
                .push_back_ptr(NonNull::new_unchecked(base.add(i)));
        }
    }
    for i in 0..CHURN_OPS {
        // SAFETY: the index is in bounds, the node is on this queue, and
        // it rejoins the queue once it has left.
        unsafe {
            let node = NonNull::new_unchecked(base.add(churn_index(i, n)));
            queue.as_mut().remove_ptr(node);
            queue.as_mut().push_front_ptr(node);
        }
    }
    queue.iter().fold(0, |acc, node| mix(acc, node.key))
}

fn concat(nodes: &mut [Node]) -> u64 {
    let mut heads = pin!([const { Queue::new() }; CONCAT_WAYS]);
    for (i, node) in nodes.iter_mut().enumerate() {
        lane(heads.as_mut(), i.wrapping_rem(CONCAT_WAYS)).push_back(node);
    }
    for w in 1..CONCAT_WAYS {
        let (first, other) = first_and(heads.as_mut(), w);
        first.append(other);
    }
    lane(heads.as_mut(), 0)
        .iter()
        .fold(0, |acc, node| mix(acc, node.key))
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
    bench(criterion, "walk", walk);
    bench(criterion, "walk_rev", walk_rev);
    bench(criterion, "churn", churn);
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
