# tailq

An intrusive, doubly linked tail queue for Rust, after BSD's
`sys/queue.h` `TAILQ`. The links live inside caller-owned nodes, so the
queue never allocates or frees. The crate is `no_std` and needs no
`alloc`.

- A two-word head and two-word links, walked both ways.
- O(1) push and pop at either end, insertion before or after a cursor,
  removal of any node given the head, replacement in place, and
  appending one queue to another.
- An opt-in length: `TailQueue<'nodes, A, Counted>` adds one word to the
  head and answers `len()` in O(1).
- `Link::is_linked()` in every build. A link of all zero bytes is
  unlinked, and a head of all zero bytes is an empty queue, so nodes and
  heads in zeroed memory need no initialisation.
- Nodes lent as `&'nodes mut Node` are handed back the same way when
  they leave, so the borrow checker keeps a linked node alive, in place
  and on one queue at a time. The `_ptr` operations work on raw pointers
  instead. The pointers they return are the ones the nodes were pushed
  with, so code that owns the nodes by pointer can write them while they
  are linked.
- Debug builds check that each link's neighbours point back at it, and
  that a pushed link is on no queue.

```rust
use core::pin::pin;
use tailq::Link;

struct Thread {
    id: u32,
    link: Link,
}

tailq::queue!(RunQueue<'nodes> = Thread { link });

let mut first = Thread { id: 1, link: Link::new() };
let mut second = Thread { id: 2, link: Link::new() };
let mut run_queue = pin!(RunQueue::new());
run_queue.as_mut().push_back(&mut first);
run_queue.as_mut().push_front(&mut second);

let ids: Vec<u32> = run_queue.iter().map(|thread| thread.id).collect();
assert_eq!(ids, [2, 1]);
```

`queue!` declares the queue type for a node type and the field that
links it. Without a lifetime the type is `TailQueue<'static, _>`, for
heads in statics whose nodes are pushed by pointer. A trailing `counted`
keeps the length:

```rust
tailq::queue!(pub RunQueue = Thread { run_queue_node });
tailq::queue!(pub Frames = Frame { link }, counted);

static mut RUN_QUEUE: RunQueue = RunQueue::new();
```

One node can sit on several queues at once by embedding one `Link` per
queue. Generic nodes implement the `Adapter` trait by hand.

The head is pinned because the first node's link points back into it.
Pin it on the stack with `core::pin::pin!`, or keep it in a `static` or
other memory that never moves, and pin it with `Pin::new_unchecked`.

## Minimum supported Rust version

Rust 1.99.

## Development

CI runs these checks on each PR:

```sh
cargo fmt --check
cargo clippy --all-targets --locked
cargo test --locked
cargo test --locked --release
cargo doc --no-deps --locked
cargo build --locked --target x86_64-unknown-none
MIRIFLAGS=-Zmiri-strict-provenance cargo +nightly miri test --locked
MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo +nightly miri test --locked
```

`Cargo.toml` denies every warning, Clippy's pedantic, nursery, cargo and restriction groups included, so these checks
need no flags. CI runs them on the toolchains pinned in `.github/actions/setup-rust` and `.github/workflows/test.yml`.
The no_std build needs its target: `rustup target add x86_64-unknown-none`.

CI also packages the crate, builds the library on Rust 1.99, the minimum, and lints the Markdown with markdownlint-cli2.

## License

MIT, see [LICENSE](LICENSE).
