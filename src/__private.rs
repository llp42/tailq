// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! What [`adapter!`](crate::adapter!) expands to and nothing else may
//! name.

pub use crate::adapter::check_payload;
// What the expansion of `adapter!` names from `core`, named through
// `$crate`: a path `::core::...` would resolve in the caller's crate,
// which can put a crate of its own under that name.
pub use core::clone::Clone;
pub use core::convert::Infallible;
pub use core::fmt::Debug;
pub use core::marker::{Copy, PhantomData, Sized};
pub use core::mem::{MaybeUninit, offset_of};
pub use core::prelude::v1::derive;
pub use core::primitive::usize;

use crate::Link;

/// Seals [`Adapter`](crate::Adapter): only [`adapter!`](crate::adapter!)
/// implements it.
pub trait Sealed {}

/// Returns whether a field `offset` bytes into every `Node` is aligned for
/// a `Field`: its offset is, and every node is aligned at least as much.
#[must_use]
#[inline]
pub const fn aligned<Node, Field>(offset: usize) -> bool {
    let align = align_of::<Field>();
    offset.is_multiple_of(align) && align_of::<Node>() >= align
}

/// Returns `offset`, the offset of a node's link, after checking it.
///
/// The link's place, `at` bytes into the node, must lie there, and the
/// link must be aligned in every `Node`.  The check runs here, where
/// `core` is the real one, not in the caller's crate, which could shadow
/// it.
///
/// # Panics
///
/// At compile time, when the link's place lies elsewhere, which a path
/// through a deref makes it do, or when the link could be unaligned.
#[must_use]
#[inline]
pub const fn link_offset<Node>(offset: usize, at: isize) -> usize {
    assert!(
        at >= 0 && at.cast_unsigned() == offset,
        "tailq::adapter!: the path reaches the link through a deref",
    );
    assert!(
        aligned::<Node, Link>(offset),
        "tailq::adapter!: the link is not aligned in every node",
    );
    offset
}

/// Returns `offset`, the offset of a node's payload, after checking it
/// as [`link_offset`] checks a link's.
///
/// # Panics
///
/// At compile time, when the payload's place lies elsewhere, or when the
/// payload could be unaligned.
#[must_use]
#[inline]
pub const fn payload_offset<Node, Payload>(offset: usize, at: isize) -> usize {
    assert!(
        at >= 0 && at.cast_unsigned() == offset,
        "tailq::adapter!: the path reaches the payload through a deref",
    );
    assert!(
        aligned::<Node, Payload>(offset),
        "tailq::adapter!: the payload is not aligned in every node",
    );
    offset
}
