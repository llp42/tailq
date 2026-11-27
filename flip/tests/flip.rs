// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! A type macro expands wherever `adapter!` pastes the node type, so if it
//! pasted it more than once, a macro answering differently each time could
//! make `Node` one type and `OFFSET` another's, from safe code.  Each
//! adapter here is declared with `flip!`, which names `Fake` at one
//! expansion and `Real` at every other; each must measure the node it
//! declares, wherever the odd answer falls.

#![forbid(unsafe_code)]

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

// A payload adapter's node type is pasted once too.

/// A payload node whose link and payload are not at its start.
#[repr(C)]
struct RealKeyed {
    pad: [u64; 2],
    link: Link,
    key: u64,
}

/// One whose link and payload are, apart from each other.
#[repr(C)]
struct FakeKeyed {
    link: Link,
    key: u64,
    pad: [u64; 2],
}

// The first two expansions are the struct's field and derive(Clone)'s check
// of it; the third and fourth, the node type and the payload rule's check
// that its node is the adapter's, reject `FakeKeyed`.  Any expansion past
// them must measure the node the adapter declares.
tailq::adapter!(KeyedFirst = flip!(keyed_first; FakeKeyed, RealKeyed) { link, mut key: u64 });
tailq::adapter!(KeyedFifth = flip!(keyed_fifth;
    RealKeyed, RealKeyed, RealKeyed, RealKeyed, FakeKeyed, RealKeyed
) { link, mut key: u64 });
tailq::adapter!(KeyedSixth = flip!(keyed_sixth;
    RealKeyed, RealKeyed, RealKeyed, RealKeyed, RealKeyed, FakeKeyed, RealKeyed
) { link, mut key: u64 });
tailq::adapter!(KeyedEighth = flip!(keyed_eighth;
    RealKeyed, RealKeyed, RealKeyed, RealKeyed, RealKeyed, RealKeyed, RealKeyed, FakeKeyed, RealKeyed
) { link, mut key: u64 });

/// Returns whether the adapter measures the link and the payload of the
/// node it declares.
const fn measures_its_payload<A>(link: usize, payload: usize) -> bool
where
    A: tailq::Payload,
{
    A::OFFSET == link && A::PAYLOAD_OFFSET == payload
}

#[test]
fn a_payload_adapter_measures_the_node_it_declares() {
    assert!(measures_its_payload::<KeyedFirst>(
        offset_of!(<KeyedFirst as Adapter>::Node, link),
        offset_of!(<KeyedFirst as Adapter>::Node, key)
    ));
    assert!(measures_its_payload::<KeyedFifth>(
        offset_of!(<KeyedFifth as Adapter>::Node, link),
        offset_of!(<KeyedFifth as Adapter>::Node, key)
    ));
    assert!(measures_its_payload::<KeyedSixth>(
        offset_of!(<KeyedSixth as Adapter>::Node, link),
        offset_of!(<KeyedSixth as Adapter>::Node, key)
    ));
    assert!(measures_its_payload::<KeyedEighth>(
        offset_of!(<KeyedEighth as Adapter>::Node, link),
        offset_of!(<KeyedEighth as Adapter>::Node, key)
    ));
    let mut node = RealKeyed {
        pad: [0; 2],
        link: Link::new(),
        key: 1,
    };
    let mut queue = TailQueue::<'_, KeyedFifth>::new();
    queue.push_back(&mut node);
    if let Some(key) = queue.front_mut() {
        *key = 2;
    }
    assert_eq!(
        queue.front().map(|front| (front.key, front.pad)),
        Some((2, [0; 2]))
    );
    assert_eq!(size_of::<RealKeyed>(), size_of::<FakeKeyed>());
}
