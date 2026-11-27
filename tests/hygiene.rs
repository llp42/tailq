// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! `adapter!` in a crate that puts itself under the name `core`.  The
//! third review found that the macro named `::core`, which resolves in the
//! caller's crate: a crate with an `offset_of!` that lied then made an
//! adapter step 4096 bytes into a 24-byte node, from safe code, and one
//! with an `assert!` that checked nothing let a packed node through.  The
//! macro names everything through `$crate` now.

#![forbid(unsafe_code)]
#![expect(
    clippy::tests_outside_test_module,
    reason = "an integration test crate is itself the test module"
)]

// The macro must not resolve anything through this crate: if its
// expansion named `::core` again, this crate would be used and the
// expectation unfulfilled, and the expansion would not find `core::mem`.
#[expect(
    unused_extern_crates,
    reason = "adapter! must name nothing through the caller's `core`"
)]
extern crate self as core;

use tailq::{Adapter, Link, Payload, TailQueue};

/// A node whose link is not its first field.
struct Node {
    key: u64,
    link: Link,
    hits: u32,
}

tailq::adapter!(NodeLink = Node { link, mut hits: u32 });

#[test]
fn adapters_ignore_a_crate_named_core() {
    assert_eq!(
        (
            <NodeLink as Adapter>::OFFSET,
            <NodeLink as Payload>::PAYLOAD_OFFSET
        ),
        (
            std::mem::offset_of!(Node, link),
            std::mem::offset_of!(Node, hits)
        )
    );
    let mut nodes = [1, 2].map(|key| Node {
        key,
        link: Link::new(),
        hits: 0,
    });
    let mut queue: TailQueue<'_, NodeLink> = nodes.iter_mut().collect();
    for hits in &mut queue {
        *hits = hits.wrapping_add(1);
    }
    let seen: Vec<(u64, u32)> =
        queue.iter().map(|node| (node.key, node.hits)).collect();
    assert_eq!(seen, [(1, 1), (2, 1)]);
    assert_eq!(queue.pop_back().map(|node| node.key), Some(2));
}
