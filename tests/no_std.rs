// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! `adapter!` in a `no_std` crate that puts itself under the name `std`.
//! `hygiene.rs` catches a `::core` path in the expansion, and this test a
//! `::std` one, which would fail every `no_std` user and resolve, like
//! `::core`, in the caller's crate, which can shadow it.

#![no_std]
#![forbid(unsafe_code)]

// The macro must not resolve anything through this crate: if its
// expansion named `::std`, this crate would be used and the expectation
// unfulfilled, and the expansion would not find `std::mem`.
#[expect(
    unused_extern_crates,
    reason = "adapter! must name nothing through the caller's `std`"
)]
extern crate self as std;

use tailq::{Link, TailQueue};

/// A node whose key is its payload.
struct Node {
    link: Link,
    key: u32,
}

tailq::adapter!(NodeLink = Node { link, mut key: u32 });

#[test]
fn adapters_need_no_std() {
    let mut nodes = [1, 2].map(|key| Node {
        link: Link::new(),
        key,
    });
    let mut queue: TailQueue<'_, NodeLink> = nodes.iter_mut().collect();
    for key in &mut queue {
        *key = key.wrapping_add(10);
    }
    assert_eq!(queue.pop_back().map(|node| node.key), Some(12));
    assert_eq!(queue.front().map(|node| node.key), Some(11));
}
