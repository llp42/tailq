// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! The fifth review found that `adapter!` pasted its node type at eight
//! places, and a type macro expands at each: a macro answering differently
//! each time made `Node` one type and `OFFSET` another's, from safe code.
//! Each adapter here is declared with `flip!`, which names `Fake` at one
//! expansion and `Real` at every other; each must measure the node it
//! declares, wherever the odd answer falls.

#![forbid(unsafe_code)]
#![expect(
    clippy::tests_outside_test_module,
    reason = "an integration test crate is itself the test module"
)]

use core::mem::offset_of;
use tailq::{Adapter, Link, TailQueue};
use tailq_flip::flip;

/// A node whose link is not at its start.
#[repr(C)]
struct Real {
    pad: [u64; 2],
    link: Link,
}

/// A node whose link is.
#[repr(C)]
struct Fake {
    link: Link,
    pad: [u64; 2],
}

tailq::adapter!(First = flip!(first; Fake, Real) { link });
tailq::adapter!(Second = flip!(second; Real, Fake, Real) { link });
tailq::adapter!(Third = flip!(third; Real, Real, Fake, Real) { link });
tailq::adapter!(Seventh = flip!(seventh; Real, Real, Real, Real, Real, Real, Fake, Real) { link });
tailq::adapter!(Eighth = flip!(eighth; Real, Real, Real, Real, Real, Real, Real, Fake, Real) { link });

/// Returns whether the adapter measures the link of the node it declares.
const fn measures_its_node<A>(offset: usize) -> bool
where
    A: Adapter,
{
    A::OFFSET == offset
}

#[test]
fn an_adapter_measures_the_node_it_declares() {
    assert!(measures_its_node::<First>(offset_of!(
        <First as Adapter>::Node,
        link
    )));
    assert!(measures_its_node::<Second>(offset_of!(
        <Second as Adapter>::Node,
        link
    )));
    assert!(measures_its_node::<Third>(offset_of!(
        <Third as Adapter>::Node,
        link
    )));
    assert!(measures_its_node::<Seventh>(offset_of!(
        <Seventh as Adapter>::Node,
        link
    )));
    assert!(measures_its_node::<Eighth>(offset_of!(
        <Eighth as Adapter>::Node,
        link
    )));
    assert!(TailQueue::<Third>::new().is_empty());
    assert_eq!(size_of::<Real>(), size_of::<Fake>());
}
