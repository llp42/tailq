// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Whether a queue keeps its length.

use core::fmt;
use core::mem;

/// How a [`TailQueue`](crate::TailQueue) keeps its length: [`Counted`] or [`Uncounted`].
///
/// The trait is sealed; this crate's two types are its only
/// implementations.
pub trait Count: Sealed + fmt::Debug {}

/// The [`Count`] of a queue that keeps its length in its head, one word,
/// for an O(1) [`len`](crate::TailQueue::len).
#[derive(Debug, Clone, Copy)]
pub struct Counted(usize);

impl Counted {
    /// Returns the length.
    pub(crate) const fn get(self) -> usize {
        self.0
    }
}

/// The [`Count`] of a queue that keeps no length, so its head stays two
/// words.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Uncounted;

impl Count for Counted {}
impl Count for Uncounted {}

/// The length operations behind [`Count`], private so
/// no other crate implements it.
#[expect(
    unnameable_types,
    reason = "a supertrait no other crate can name is what seals `Count`"
)]
pub trait Sealed: Sized {
    /// The length of an empty queue, all zero bytes.
    const ZERO: Self;

    /// Adds `n` to the length.
    fn add(&mut self, n: usize);

    /// Subtracts one from the length.
    fn sub_one(&mut self);

    /// Returns the length and sets it to zero.
    fn take(&mut self) -> usize;
}

impl Sealed for Counted {
    const ZERO: Self = Self(0);

    #[inline]
    fn add(&mut self, n: usize) {
        self.0 = self.0.wrapping_add(n);
    }

    #[inline]
    fn sub_one(&mut self) {
        self.0 = self.0.wrapping_sub(1);
    }

    #[inline]
    fn take(&mut self) -> usize {
        mem::take(&mut self.0)
    }
}

impl Sealed for Uncounted {
    const ZERO: Self = Self;

    #[inline]
    fn add(&mut self, _: usize) {}

    #[inline]
    fn sub_one(&mut self) {}

    #[inline]
    fn take(&mut self) -> usize {
        0
    }
}
