// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! `adapter!` in a crate that forbids lints.  The ninth review found that
//! a caller forbidding a lint the expansion allowed, dead code or
//! single-use lifetimes, could declare no adapter, since `forbid`
//! overrules an `allow`.  The expansion allows nothing now.

#![forbid(dead_code, single_use_lifetimes, unsafe_code)]
#![expect(
    clippy::tests_outside_test_module,
    reason = "an integration test crate is itself the test module"
)]

use tailq::{Link, TailQueue};

/// A node borrowing a name, with a payload.
struct Named<'name> {
    link: Link,
    name: &'name str,
    hits: u32,
}

tailq::adapter!(NamedLink<'name> = Named<'name> { link, mut hits: u32 });

#[test]
fn adapters_declare_under_a_forbid() {
    let mut named = Named {
        link: Link::new(),
        name: "a",
        hits: 0,
    };
    let mut queue = TailQueue::<'_, NamedLink<'_>>::new();
    queue.push_back(&mut named);
    if let Some(hits) = queue.front_mut() {
        *hits = hits.wrapping_add(1);
    }
    assert_eq!(
        queue.front().map(|front| (front.name, front.hits)),
        Some(("a", 1))
    );
}
