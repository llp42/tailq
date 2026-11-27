// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! What [`adapter!`](crate::adapter!) expands to and nothing else may
//! name.

pub use crate::adapter::check_payload;
// What the expansion of `adapter!` names from `core`, named through
// `$crate`: a path `::core::...` would resolve in the caller's crate,
// which can put a crate of its own under that name.
pub use core::convert::Infallible;
pub use core::marker::PhantomData;
pub use core::mem::{MaybeUninit, offset_of};

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

/// Returns `offset`, the offset of a node's link, after checking that the
/// link is aligned in every `Node`.  The check runs here, where `core` is
/// the real one, not in the caller's crate, which could shadow it.
///
/// # Panics
///
/// At compile time, when the link could be unaligned.
#[must_use]
#[inline]
pub const fn link_offset<Node>(offset: usize) -> usize {
    assert!(
        aligned::<Node, Link>(offset),
        "tailq::adapter!: the link is not aligned in every node",
    );
    offset
}

/// Returns `offset`, the offset of a node's payload, after checking that
/// the payload is aligned in every `Node`, as [`link_offset`] does.
///
/// # Panics
///
/// At compile time, when the payload could be unaligned.
#[must_use]
#[inline]
pub const fn payload_offset<Node, Payload>(offset: usize) -> usize {
    assert!(
        aligned::<Node, Payload>(offset),
        "tailq::adapter!: the payload is not aligned in every node",
    );
    offset
}
