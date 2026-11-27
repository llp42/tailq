// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! How a queue finds a node's link: the adapter trait, the adapter
//! [`queue!`](crate::queue!) declares, and the macro.

use super::Link;
use core::convert::Infallible;
use core::fmt;
use core::marker::PhantomData;
use core::ptr::NonNull;

/// Maps a node type to the [`Link`] it embeds, and back.
///
/// [`queue!`](crate::queue!) implements it through [`Field`]; implement it
/// by hand for what the macro cannot name, such as a generic node.  An
/// adapter is an uninhabited type: a queue names one as its type parameter
/// and never holds a value of it.
///
/// ```
/// use core::marker::PhantomData;
/// use core::ptr::NonNull;
/// use tailq::{Adapter, Link, TailQueue};
///
/// struct Thread<P> {
///     platform: PhantomData<P>,
///     link: Link,
/// }
///
/// struct ThreadLink<P>(core::convert::Infallible, PhantomData<P>);
///
/// // SAFETY: both directions step by the offset of `link`, a `Link`
/// // field, from the pointer they are given.
/// unsafe impl<P> Adapter for ThreadLink<P> {
///     type Node = Thread<P>;
///
///     unsafe fn link(node: NonNull<Thread<P>>) -> NonNull<Link> {
///         unsafe { NonNull::new_unchecked(&raw mut (*node.as_ptr()).link) }
///     }
///
///     unsafe fn node(link: NonNull<Link>) -> NonNull<Thread<P>> {
///         let offset = core::mem::offset_of!(Thread<P>, link);
///         unsafe { link.byte_sub(offset).cast() }
///     }
/// }
///
/// type RunQueue<'a, P> = TailQueue<'a, ThreadLink<P>>;
/// ```
///
/// # Safety
///
/// `link` must return a pointer to a [`Link`] inside the node it is
/// given, at the same offset for every node, and `node` must invert it;
/// both must derive their result from the pointer they are given, so that
/// it keeps that pointer's access.
pub unsafe trait Adapter {
    /// The node type that embeds the link.
    type Node;

    /// Returns the link embedded in `node`.
    ///
    /// # Safety
    ///
    /// `node` must point at a live node.
    unsafe fn link(node: NonNull<Self::Node>) -> NonNull<Link>;

    /// Returns the node that embeds `link`.
    ///
    /// # Safety
    ///
    /// `link` must be the link of a live node.
    unsafe fn node(link: NonNull<Link>) -> NonNull<Self::Node>;
}

/// Vouches that a [`Link`] lies `OFFSET` bytes into every `Self`, so that
/// [`Field<Self, OFFSET>`](Field) is an [`Adapter`].  [`queue!`](crate::queue!)
/// implements it.
///
/// # Safety
///
/// Every `Self` must hold a [`Link`] `OFFSET` bytes in.
pub unsafe trait LinkAt<const OFFSET: usize> {}

/// The [`Adapter`] for the [`Link`] `OFFSET` bytes into a `T`, the one
/// [`queue!`](crate::queue!) names.
pub struct Field<T, const OFFSET: usize>(Infallible, PhantomData<fn() -> T>);

// SAFETY: `T: LinkAt<OFFSET>` puts a link `OFFSET` bytes into every `T`,
// and both directions step by that offset from the pointer they are given.
unsafe impl<T, const OFFSET: usize> Adapter for Field<T, OFFSET>
where
    T: LinkAt<OFFSET>,
{
    type Node = T;

    #[inline]
    unsafe fn link(node: NonNull<T>) -> NonNull<Link> {
        unsafe { node.byte_add(OFFSET).cast() }
    }

    #[inline]
    unsafe fn node(link: NonNull<Link>) -> NonNull<T> {
        unsafe { link.byte_sub(OFFSET).cast() }
    }
}

impl<T, const OFFSET: usize> fmt::Debug for Field<T, OFFSET> {
    #[inline]
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {}
    }
}

/// Declares a queue type of nodes linked through one of their fields.
///
/// ```
/// use tailq::Link;
///
/// pub struct Thread {
///     id: u32,
///     run_queue_node: Link,
/// }
///
/// tailq::queue!(
///     /// The threads ready to run.
///     pub RunQueue = Thread { run_queue_node }
/// );
///
/// static mut RUN_QUEUE: RunQueue = RunQueue::new();
/// ```
///
/// The type is `TailQueue<'static, _>`, for nodes that live forever or are
/// pushed by pointer.  Naming a lifetime after the type declares
/// `TailQueue<'nodes, _>` instead, for nodes lent as `&'nodes mut`, and a
/// trailing `counted` keeps the length:
///
/// ```
/// use tailq::Link;
///
/// struct Frame {
///     number: u64,
///     link: Link,
/// }
///
/// tailq::queue!(Frames<'nodes> = Frame { link }, counted);
///
/// let mut frame = Frame { number: 7, link: Link::new() };
/// let mut frames = Frames::new();
/// frames.push_back(&mut frame);
/// assert_eq!(frames.len(), 1);
/// ```
///
/// The field must have type [`Link`]; any other type fails to compile:
///
/// ```compile_fail,E0308
/// struct Thread {
///     link: u64,
/// }
///
/// tailq::queue!(RunQueue = Thread { link });
/// ```
///
/// A link inside a nested field takes its path, `Thread { sched.link }`.
///
/// The type is `TailQueue<'_, Field<Node, OFFSET>, _>`, with `OFFSET` the
/// field's offset in the node.  Each field takes one `queue!`, since two
/// would both implement [`LinkAt`] for it; a second type through the same
/// field, with another lifetime or count, names that type in full.
/// Generic nodes implement [`Adapter`] by hand.
#[macro_export]
macro_rules! queue {
    (@count) => { $crate::Uncounted };
    (@count counted) => { $crate::Counted };
    (
        @declare [$($attr:tt)*] [$vis:vis] $name:ident [$($param:tt)*]
            $lt:lifetime, $node:ty, [$($field:ident).+], [$($count:ident)?]
    ) => {
        const _: fn(&$node) -> &$crate::Link = |node| &node.$($field).+;

        // SAFETY: the binding above proves the field is a `Link`.
        unsafe impl
            $crate::LinkAt<{ ::core::mem::offset_of!($node, $($field).+) }>
            for $node
        {
        }

        $($attr)*
        $vis type $name $($param)* = $crate::TailQueue<
            $lt,
            $crate::Field<$node, { ::core::mem::offset_of!($node, $($field).+) }>,
            $crate::queue!(@count $($count)?),
        >;
    };
    (
        $(#[$attr:meta])* $vis:vis $name:ident<$lt:lifetime>
            = $node:ty { $($field:ident).+ } $(, $count:ident)?
    ) => {
        $crate::queue!(
            @declare [$(#[$attr])*] [$vis] $name [<$lt>]
                $lt, $node, [$($field).+], [$($count)?]
        );
    };
    (
        $(#[$attr:meta])* $vis:vis $name:ident
            = $node:ty { $($field:ident).+ } $(, $count:ident)?
    ) => {
        $crate::queue!(
            @declare [$(#[$attr])*] [$vis] $name []
                'static, $node, [$($field).+], [$($count)?]
        );
    };
}
