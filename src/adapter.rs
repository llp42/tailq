// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! How a queue finds a node's link and payload: the adapter traits, which
//! only [`adapter!`](crate::adapter!) implements, and the macro.

use super::__private::Sealed;
use super::Link;
use core::mem::size_of;
use core::ptr::NonNull;

/// Maps a node type to the [`Link`] it embeds.
///
/// [`adapter!`](crate::adapter!) declares the types that implement it, and
/// nothing else may: the trait is sealed.  Code generic over queues names
/// it in bounds, `A: Adapter`, and reaches the node type as `A::Node`.
///
/// # Safety
///
/// Only [`adapter!`](crate::adapter!) implements it.  `OFFSET` is the
/// offset of an aligned [`Link`] field in every `Node`.
pub unsafe trait Adapter: Sealed {
    /// The node type that embeds the link.
    type Node;

    /// The offset of the link in the node, in bytes.
    #[doc(hidden)]
    const OFFSET: usize;
}

/// An [`Adapter`] whose nodes hold a payload apart from their link, which
/// a queue lends as `&mut` while the node is on it: through
/// [`front_mut`](crate::TailQueue::front_mut),
/// [`back_mut`](crate::TailQueue::back_mut),
/// [`iter_mut`](crate::TailQueue::iter_mut) and
/// [`current_mut`](crate::CursorMut::current_mut).
///
/// [`adapter!`](crate::adapter!) implements it when the field list names
/// the payload after the link, `Thread { link, mut info: Info }`.  A
/// queue hands out only `&A::Node` otherwise, since a `&mut` to the
/// whole node could overwrite its link.  The queue checks at compile time,
/// where it first lends a payload, that the payload lies apart from the
/// link.
///
/// A payload may hold the links of other queues.  A node is on several
/// queues only through pushes by pointer, and their contract then makes
/// the caller keep those queues off the node while this one lends its
/// payload, and keep the payload's borrower off those links.
///
/// # Safety
///
/// Only [`adapter!`](crate::adapter!) implements it.  `PAYLOAD_OFFSET` is
/// the offset of an aligned `Payload` field in every `Node`.
pub unsafe trait Payload: Adapter {
    /// The part of a node a queue lends mutably.
    type Payload;

    /// The offset of the payload in the node, in bytes.
    #[doc(hidden)]
    const PAYLOAD_OFFSET: usize;
}

/// Returns the link embedded in `node`.
///
/// # Safety
///
/// `node` must point at a live node.
#[inline]
pub(crate) const unsafe fn link_of<A>(node: NonNull<A::Node>) -> NonNull<Link>
where
    A: Adapter,
{
    // SAFETY: the link lies `OFFSET` bytes into the live node.
    unsafe { node.byte_add(A::OFFSET).cast() }
}

/// Returns the node that embeds `link`.
///
/// # Safety
///
/// `link` must be the link of a live node.
#[inline]
pub(crate) const unsafe fn node_of<A>(link: NonNull<Link>) -> NonNull<A::Node>
where
    A: Adapter,
{
    // SAFETY: the link lies `OFFSET` bytes into its live node.
    unsafe { link.byte_sub(A::OFFSET).cast() }
}

/// Checks, in a constant, that the payload of `A`'s nodes lies apart from
/// their link.
///
/// # Panics
///
/// At compile time, when the payload overlaps the link.
#[inline]
pub const fn check_payload<A>()
where
    A: Payload,
{
    let (link, payload) = (A::OFFSET, A::PAYLOAD_OFFSET);
    assert!(
        payload.saturating_add(size_of::<A::Payload>()) <= link
            || link.saturating_add(size_of::<Link>()) <= payload,
        "tailq: the payload overlaps the link",
    );
}

/// Returns the payload of `node`, after checking at compile time that it
/// lies apart from the link.
///
/// # Safety
///
/// `node` must point at a live node.
#[inline]
pub(crate) const unsafe fn payload_of<A>(
    node: NonNull<A::Node>,
) -> NonNull<A::Payload>
where
    A: Payload,
{
    const { check_payload::<A>() };
    // SAFETY: the payload lies `PAYLOAD_OFFSET` bytes into the live node.
    unsafe { node.byte_add(A::PAYLOAD_OFFSET).cast() }
}

/// Declares an [`Adapter`]: an uninhabited type that links a node type
/// through one of its fields.
///
/// ```
/// use tailq::{Counted, Link, TailQueue};
///
/// pub struct Thread {
///     id: u32,
///     run_queue_node: Link,
/// }
///
/// tailq::adapter!(
///     /// Links a thread into the queue of threads ready to run.
///     pub RunLink = Thread { run_queue_node }
/// );
///
/// /// The threads ready to run.
/// pub type RunQueue<'nodes> = TailQueue<'nodes, RunLink>;
/// /// The same, with an O(1) length.
/// pub type CountedRunQueue<'nodes> = TailQueue<'nodes, RunLink, Counted>;
///
/// let mut thread = Thread { id: 7, run_queue_node: Link::new() };
/// let mut run_queue = CountedRunQueue::new();
/// run_queue.push_back(&mut thread);
/// assert_eq!(run_queue.len(), 1);
/// ```
///
/// One node joins several queues at once through several fields, each
/// with its adapter.  A link inside a nested field takes its path,
/// `Thread { sched.link }`.  A generic node takes the adapter's
/// parameters after its name, lifetimes, types and consts, and their
/// bounds in a trailing `where`:
///
/// ```
/// use core::marker::PhantomData;
/// use tailq::{Link, TailQueue};
///
/// trait Platform {}
///
/// struct Sched {
///     link: Link,
/// }
///
/// struct Thread<'name, P: Platform, const CPUS: usize> {
///     name: &'name str,
///     affinity: [bool; CPUS],
///     platform: PhantomData<P>,
///     sched: Sched,
/// }
///
/// tailq::adapter!(
///     SchedLink<'name, P, const CPUS: usize> = Thread<'name, P, CPUS> {
///         sched.link
///     } where P: Platform
/// );
///
/// type RunQueue<'nodes, 'name, P, const CPUS: usize> =
///     TailQueue<'nodes, SchedLink<'name, P, CPUS>>;
/// ```
///
/// A queue whose head lives in a `static` takes `'static` nodes, such as
/// leaked ones, or nodes pushed by pointer, and is reached through a lock:
///
/// ```
/// use std::sync::Mutex;
/// use tailq::{Link, TailQueue};
///
/// struct Thread {
///     id: u32,
///     link: Link,
/// }
///
/// tailq::adapter!(RunLink = Thread { link });
///
/// static RUN_QUEUE: Mutex<TailQueue<'static, RunLink>> =
///     Mutex::new(TailQueue::new());
///
/// let thread = Box::leak(Box::new(Thread { id: 1, link: Link::new() }));
/// let mut run_queue = RUN_QUEUE.lock().unwrap();
/// run_queue.push_back(thread);
/// assert_eq!(run_queue.front().map(|thread| thread.id), Some(1));
/// ```
///
/// A payload, which the queue lends as `&mut` while the node is linked,
/// follows the link as `mut path: Type`, and makes the adapter a
/// [`Payload`] one:
///
/// ```
/// use tailq::{Link, TailQueue};
///
/// struct Info {
///     id: u32,
///     runtime: u64,
/// }
///
/// struct Thread {
///     link: Link,
///     info: Info,
/// }
///
/// tailq::adapter!(RunLink = Thread { link, mut info: Info });
///
/// let mut threads = [1, 2].map(|id| Thread {
///     link: Link::new(),
///     info: Info { id, runtime: 0 },
/// });
/// let mut run_queue: TailQueue<'_, RunLink> = threads.iter_mut().collect();
/// for info in run_queue.iter_mut() {
///     info.runtime += u64::from(info.id);
/// }
/// if let Some(info) = run_queue.front_mut() {
///     info.runtime *= 10;
/// }
/// let runtimes: Vec<u64> = run_queue.iter().map(|t| t.info.runtime).collect();
/// assert_eq!(runtimes, [10, 2]);
/// ```
///
/// The payload must not overlap the link, which the queue checks at
/// compile time where it first lends a payload:
///
/// ```compile_fail,E0080
/// use tailq::{Link, TailQueue};
///
/// struct Sched {
///     link: Link,
///     runtime: u64,
/// }
///
/// struct Thread {
///     sched: Sched,
/// }
///
/// tailq::adapter!(RunLink = Thread { sched.link, mut sched: Sched });
///
/// let mut queue = TailQueue::<RunLink>::new();
/// let _ = queue.front_mut();
/// ```
///
/// The field must have type [`Link`] itself; any other type fails to
/// compile, including one that only derefs to a link:
///
/// ```compile_fail,E0308
/// struct Thread {
///     link: u64,
/// }
///
/// tailq::adapter!(RunLink = Thread { link });
/// ```
///
/// ```compile_fail,E0308
/// use tailq::Link;
///
/// struct Thread {
///     link: Box<Link>,
/// }
///
/// tailq::adapter!(RunLink = Thread { link });
/// ```
///
/// A packed node, whose link could be unaligned, fails to compile too:
///
/// ```compile_fail,E0080
/// use tailq::Link;
///
/// #[repr(C, packed)]
/// struct Thread {
///     tag: u8,
///     link: Link,
/// }
///
/// tailq::adapter!(RunLink = Thread { link });
/// ```
#[macro_export]
macro_rules! adapter {
    ($(#[$attr:meta])* $vis:vis $name:ident $($rest:tt)*) => {
        $crate::__adapter!(@start [$(#[$attr])* $vis $name] $($rest)*);
    };
}

/// The rules behind [`adapter!`]: it reads the adapter's parameters one at
/// a time, then declares the adapter and implements its traits.
#[doc(hidden)]
#[macro_export]
macro_rules! __adapter {
    (@start $head:tt = $($rest:tt)*) => {
        $crate::__adapter!(@define $head [] [] = $($rest)*);
    };
    (@start $head:tt < $($rest:tt)*) => {
        $crate::__adapter!(@params $head [] [] $($rest)*);
    };
    (@params $head:tt [$($decl:tt)*] [$($arg:tt)*] > $($rest:tt)*) => {
        $crate::__adapter!(@define $head [$($decl)*] [$($arg)*] $($rest)*);
    };
    (@params $head:tt [$($decl:tt)*] [$($arg:tt)*] , $($rest:tt)*) => {
        $crate::__adapter!(@params $head [$($decl)*] [$($arg)*] $($rest)*);
    };
    (
        @params $head:tt [$($decl:tt)*] [$($arg:tt)*]
            $lifetime:lifetime $($rest:tt)*
    ) => {
        $crate::__adapter!(
            @params $head [$($decl)* $lifetime,] [$($arg)* $lifetime,]
                $($rest)*
        );
    };
    (
        @params $head:tt [$($decl:tt)*] [$($arg:tt)*]
            const $const:ident : $ty:ty , $($rest:tt)*
    ) => {
        $crate::__adapter!(
            @params $head [$($decl)* const $const: $ty,] [$($arg)* $const,]
                $($rest)*
        );
    };
    (
        @params $head:tt [$($decl:tt)*] [$($arg:tt)*]
            const $const:ident : $ty:ty > $($rest:tt)*
    ) => {
        $crate::__adapter!(
            @params $head [$($decl)* const $const: $ty,] [$($arg)* $const,]
                > $($rest)*
        );
    };
    (
        @params $head:tt [$($decl:tt)*] [$($arg:tt)*]
            $param:ident $($rest:tt)*
    ) => {
        $crate::__adapter!(
            @params $head [$($decl)* $param,] [$($arg)* $param,] $($rest)*
        );
    };
    (
        @define [$(#[$attr:meta])* $vis:vis $name:ident]
            [$($decl:tt)*] [$($arg:tt)*]
            = $node:ty {
                $($field:ident).+
                $(, mut $($payload:ident).+ : $payload_ty:ty)?
                $(,)?
            }
            $(where $($bound:tt)+)?
    ) => {
        // The bounds sit on the type too, which ends at them: a brace
        // group among them fails to parse, rather than closing a header
        // below and taking the place of its body.
        $(#[$attr])*
        #[derive(Clone, Copy, Debug)]
        $vis struct $name<$($decl)*>(
            ::core::convert::Infallible,
            ::core::marker::PhantomData<fn() -> $node>,
        )
        $(where $($bound)+)?;

        // A lifetime parameter shows once in these headers.
        #[allow(single_use_lifetimes)]
        impl<$($decl)*> $crate::__private::Sealed for $name<$($arg)*>
        $(where $($bound)+)?
        {
        }

        // SAFETY: `OFFSET` is the offset of the field, which the raw place
        // proves is a `Link` and the assertion proves is aligned.
        unsafe impl<$($decl)*> $crate::Adapter for $name<$($arg)*>
        $(where $($bound)+)?
        {
            type Node = $node;

            const OFFSET: usize = {
                let mut slot = ::core::mem::MaybeUninit::<$node>::uninit();
                let node = slot.as_mut_ptr();
                // A raw place, which is never deref-coerced: rejects a
                // field that only derefs to a link.
                // SAFETY: the place is a field of the slot, and nothing
                // reads it.
                let _: *mut $crate::Link =
                    unsafe { &raw mut (*node).$($field).+ };
                let offset = ::core::mem::offset_of!($node, $($field).+);
                // Aligned in every node: rejects a packed node that could
                // leave the link unaligned.
                ::core::assert!(
                    $crate::__private::aligned::<$node, $crate::Link>(offset),
                    "tailq::adapter!: the link is not aligned in every node",
                );
                offset
            };
        }

        $crate::__adapter!(@check [$($decl)*] $name);
        $crate::__adapter!(
            @payload $name [$($decl)*] [$($arg)*] [$($($bound)+)?] $node,
                [$($($payload).+ : $payload_ty)?]
        );
    };
    // An adapter with no parameters is checked where it is declared; a
    // generic one where it is first used.
    (@check [] $name:ident) => {
        const _: usize = <$name as $crate::Adapter>::OFFSET;
    };
    (@check [$($decl:tt)+] $name:ident) => {};
    (@check_payload [] $name:ident) => {
        const _: () = $crate::__private::check_payload::<$name>();
    };
    (@check_payload [$($decl:tt)+] $name:ident) => {};
    (
        @payload $name:ident [$($decl:tt)*] [$($arg:tt)*] [$($bound:tt)*]
            $node:ty, []
    ) => {};
    (
        @payload $name:ident [$($decl:tt)*] [$($arg:tt)*] [$($bound:tt)*]
            $node:ty, [$($payload:ident).+ : $payload_ty:ty]
    ) => {
        // SAFETY: `PAYLOAD_OFFSET` is the offset of the field of the
        // adapter's node, which the raw place proves is a `$payload_ty` and
        // the assertion proves is aligned; the queue checks that it lies
        // apart from the link.
        #[allow(single_use_lifetimes)]
        unsafe impl<$($decl)*> $crate::Payload for $name<$($arg)*>
        where
            $($bound)*
        {
            type Payload = $payload_ty;

            const PAYLOAD_OFFSET: usize = {
                let mut slot = ::core::mem::MaybeUninit::<$node>::uninit();
                let node = slot.as_mut_ptr();
                // The adapter's own node.
                let _: *mut <Self as $crate::Adapter>::Node = node;
                // A raw place, which is never deref-coerced: proves the
                // field's type.
                // SAFETY: the place is a field of the slot, and nothing
                // reads it.
                let _: *mut $payload_ty =
                    unsafe { &raw mut (*node).$($payload).+ };
                let offset = ::core::mem::offset_of!($node, $($payload).+);
                // Aligned in every node: rejects a packed field that could
                // leave the payload unaligned.
                ::core::assert!(
                    $crate::__private::aligned::<$node, $payload_ty>(offset),
                    "tailq::adapter!: the payload is not aligned in every node",
                );
                offset
            };
        }

        $crate::__adapter!(@check_payload [$($decl)*] $name);
    };
}
