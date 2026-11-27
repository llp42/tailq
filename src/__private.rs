// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! What [`adapter!`](crate::adapter!) expands to and nothing else may
//! name.

pub use crate::adapter::check_payload;

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
