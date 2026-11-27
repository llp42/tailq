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
- A payload adapter, `adapter!(RunLink = Thread { link, mut info: Info })`,
  lends each linked node's payload as `&mut Info` through `front_mut`,
  `back_mut`, `iter_mut` and `CursorMut::current_mut`, and checks at
  compile time that the payload lies apart from the link.
- Debug builds check, before an operation writes anything, that each
  link it touches is consistent with its neighbours, that a node named
  by address is on this queue, and that a pushed link is on no queue,
  on a queue whose links are as its operations left them: `unsafe` code
  that corrupts a queue may get past them.
  `contains_ptr` and `cursor_mut_find_ptr` look a node up by address
  in safe code.
- A counted queue's iterators are `ExactSizeIterator`.

```rust
use tailq::{Link, TailQueue};

struct Thread {
    id: u32,
    link: Link,
}

tailq::adapter!(RunLink = Thread { link });
type RunQueue<'nodes> = TailQueue<'nodes, RunLink>;

let mut first = Thread { id: 1, link: Link::new() };
let mut second = Thread { id: 2, link: Link::new() };
let mut run_queue = RunQueue::new();
run_queue.push_back(&mut first);
run_queue.push_front(&mut second);

let ids: Vec<u32> = run_queue.iter().map(|thread| thread.id).collect();
assert_eq!(ids, [2, 1]);
```

`adapter!` declares the type that links a node type through one of its
fields, and a queue type names it. A trailing `Counted` parameter keeps
the length. A generic node takes the adapter's type parameters, with
their bounds in a `where`:

```rust
tailq::adapter!(pub RunLink = Thread { run_queue_node });
pub type RunQueue<'nodes> = TailQueue<'nodes, RunLink>;
pub type Frames<'nodes> = TailQueue<'nodes, FrameLink, Counted>;

tailq::adapter!(
    pub SchedLink<'name, P, const CPUS: usize> = Thread<'name, P, CPUS> { sched.link }
    where P: Platform
);

static RUN_QUEUE: Mutex<RunQueue<'static>> = Mutex::new(RunQueue::new());
```

The field must be a `Link` itself: one that only derefs to a link, or
sits unaligned in a packed node, fails to compile. Only `adapter!`
implements the `Adapter` trait. One node can sit on several
queues at once by embedding one `Link` per queue, each with its adapter.

Nothing points into the head: the first node's link holds a marker in
its place. So a head moves like any other value, full or empty, and it
is mutated through `&mut self`, with no pinning.

## Minimum supported Rust version

Rust 1.99. The crate is written in the 2024 edition, and `adapter!`, which expands in the caller's crate, is tested
only in crates of the 2024 edition.

## Development

CI runs these checks on each PR. Every test binary runs through `.github/limit.sh`, the runner `.cargo/config.toml`
names, which allows it 512 MiB of data memory and a timeout after cargo builds it without the limit; `tests/limits.rs`
fails when a test runs without it. Miri runs its own runner, so it runs under `limit.sh` as a whole:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked --release
python3 -I .github/check_workflows.py
cargo doc --no-deps --locked
cargo build --locked --target x86_64-unknown-none
cargo test --locked
cargo test --locked --release
cargo test --locked -p tailq-flip
cargo run --locked --release -p tailq-bench
nightly=+nightly-2026-10-04
miri=".github/limit.sh cargo $nightly miri test --locked"
strict=-Zmiri-strict-provenance
cargo $nightly miri setup  # Miri's sysroot, built without the limit, which it outgrows
cargo $nightly test --locked --doc -p tailq -p tailq-flip
MIRIFLAGS="$strict -Zmiri-symbolic-alignment-check" $miri
MIRIFLAGS="$strict -Zmiri-tree-borrows" $miri
MIRIFLAGS=$strict $miri --release
MIRIFLAGS=$strict $miri --target i686-unknown-linux-gnu
MIRIFLAGS="$strict -Zmiri-tree-borrows" $miri --target s390x-unknown-linux-gnu
```

`Cargo.toml` denies every warning, and Clippy's pedantic, nursery and cargo groups, so these checks need no flags. CI
runs them on the toolchains pinned in `.github/actions/setup-rust` and `.github/workflows/test.yml`.
The no_std build needs its target: `rustup target add x86_64-unknown-none`. The doctests run on nightly as well,
because only nightly rustdoc checks the error code of a `compile_fail` block. Miri runs on a 32-bit and a big-endian
target too, and runs the thread tests under sixteen seeds, one process each.

Coverage must be complete: every code region of `src/` runs in the tests, in some instantiation of its generic code.
`.github/coverage.py` checks it on the JSON of `cargo llvm-cov report --json`, after the tests ran under
`cargo llvm-cov show-env`.

`tests/regressions.rs` holds one test per guarantee, each named after what it pins, `tests/misuse.rs` holds the misuse
that debug builds catch, and `src/compile_fail.rs` what must not compile. The tests that need `flip/`, a type
macro whose answer changes from one expansion to the next, live in that crate, so that `tailq` does not depend on it:
they check that `adapter!` names each type once.

The benches are a program of their own, `bench/`, so that criterion is no dependency of `tailq`: `cargo run
--release -p tailq-bench -- --bench` times them, and without `--bench` runs each one once.

CI also packages the crate, builds the library on Rust 1.99.0, the minimum, and lints the Markdown with
markdownlint-cli2.

## License

MIT, see [LICENSE](LICENSE).
