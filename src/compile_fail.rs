// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Misuse the borrow checker and the type system must reject, each block
//! a `compile_fail` doctest with its error code.  Stable rustdoc only
//! checks that a block fails; nightly also checks the code, so CI runs
//! these on nightly.
//!
//! A node that does not live as long as the queue:
//!
//! ```compile_fail,E0597
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut queue = TailQueue::<NodeLink>::new();
//! {
//!     let mut node = Node { link: Link::new() };
//!     queue.push_back(&mut node);
//! }
//! assert!(!queue.is_empty());
//! ```
//!
//! A node declared after its queue, which the queue's drop would touch
//! after the node is gone:
//!
//! ```compile_fail,E0597
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut queue = TailQueue::<NodeLink>::new();
//! let mut node = Node { link: Link::new() };
//! queue.push_back(&mut node);
//! ```
//!
//! Writing, reading or moving a node while it is linked:
//!
//! ```compile_fail,E0506
//! use tailq::{Link, TailQueue};
//! struct Node { key: u32, link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { key: 1, link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! node.key = 2;
//! assert!(!queue.is_empty());
//! ```
//!
//! ```compile_fail,E0503
//! use tailq::{Link, TailQueue};
//! struct Node { key: u32, link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { key: 1, link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! let key = node.key;
//! assert!(!queue.is_empty() && key == 1);
//! ```
//!
//! ```compile_fail,E0505
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! let moved = node;
//! assert!(!queue.is_empty() && !moved.link.is_linked());
//! ```
//!
//! One node on two queues through the safe API:
//!
//! ```compile_fail,E0499
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { link: Link::new() };
//! let mut one = TailQueue::<NodeLink>::new();
//! let mut two = TailQueue::<NodeLink>::new();
//! one.push_back(&mut node);
//! two.push_back(&mut node);
//! ```
//!
//! Appending a queue of shorter-lived nodes to a longer-lived queue:
//!
//! ```compile_fail,E0597
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut long = Node { link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut long);
//! {
//!     let mut short = Node { link: Link::new() };
//!     let mut other = TailQueue::<NodeLink>::new();
//!     other.push_back(&mut short);
//!     queue.append(&mut other);
//! }
//! assert!(!queue.is_empty());
//! ```
//!
//! Inserting a shorter-lived node through a cursor:
//!
//! ```compile_fail,E0597
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut queue = TailQueue::<NodeLink>::new();
//! {
//!     let mut short = Node { link: Link::new() };
//!     queue.cursor_front_mut().insert_after(&mut short);
//! }
//! assert!(!queue.is_empty());
//! ```
//!
//! Holding a node the queue lent across a change to the queue:
//!
//! ```compile_fail,E0502
//! use tailq::{Link, TailQueue};
//! struct Node { key: u32, link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { key: 1, link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! let front = queue.front();
//! let _ = queue.pop_front();
//! assert!(front.is_some_and(|node| node.key == 1));
//! ```
//!
//! ```compile_fail,E0502
//! use tailq::{Link, TailQueue};
//! struct Node { key: u32, link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { key: 1, link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! let mut cursor = queue.cursor_front_mut();
//! let current = cursor.current();
//! let _ = cursor.remove_current();
//! assert!(current.is_some_and(|node| node.key == 1));
//! ```
//!
//! Using the queue while a mutable cursor is alive:
//!
//! ```compile_fail,E0502
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut queue = TailQueue::<NodeLink>::new();
//! let mut cursor = queue.cursor_front_mut();
//! assert!(queue.is_empty());
//! cursor.move_next();
//! ```
//!
//! An iterator outliving its queue:
//!
//! ```compile_fail,E0597
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let iter = {
//!     let queue = TailQueue::<NodeLink>::new();
//!     queue.iter()
//! };
//! assert_eq!(iter.count(), 0);
//! ```
//!
//! Two payloads of one node, or a payload and the node, at once:
//!
//! ```compile_fail,E0499
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link, key: u32 }
//! tailq::adapter!(NodeLink = Node { link, mut key: u32 });
//! let mut node = Node { link: Link::new(), key: 1 };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! let one = queue.front_mut();
//! let two = queue.back_mut();
//! assert!(one.is_some() && two.is_some());
//! ```
//!
//! ```compile_fail,E0502
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link, key: u32 }
//! tailq::adapter!(NodeLink = Node { link, mut key: u32 });
//! let mut node = Node { link: Link::new(), key: 1 };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! let key = queue.front_mut();
//! let front = queue.front();
//! assert!(key.is_some() && front.is_some());
//! ```
//!
//! A payload iterator cannot be cloned into two that lend one payload:
//!
//! ```compile_fail,E0599
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link, key: u32 }
//! tailq::adapter!(NodeLink = Node { link, mut key: u32 });
//! let mut queue = TailQueue::<NodeLink>::new();
//! let iter = queue.iter_mut();
//! let _ = iter.clone();
//! ```
//!
//! A link cannot be copied, which would put a second link at its place on
//! the queue:
//!
//! ```compile_fail,E0599
//! use tailq::Link;
//! let link = Link::new();
//! let _ = link.clone();
//! ```
//!
//! Queues of nodes that are not `Send`, or not `Sync`, stay on their
//! thread:
//!
//! ```compile_fail,E0277
//! use std::rc::Rc;
//! use tailq::{Link, TailQueue};
//! struct Node { shared: Rc<u32>, link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let queue = TailQueue::<'static, NodeLink>::new();
//! std::thread::spawn(move || queue.is_empty());
//! ```
//!
//! ```compile_fail,E0277
//! use core::cell::Cell;
//! use tailq::{Link, TailQueue};
//! struct Node { hits: Cell<u32>, link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! static QUEUE: TailQueue<'static, NodeLink> = TailQueue::new();
//! ```
//!
//! A link field that only derefs to a link, by reference or through a
//! wrapper:
//!
//! ```compile_fail,E0308
//! use tailq::Link;
//! struct Node { link: &'static Link }
//! tailq::adapter!(NodeLink = Node { link });
//! ```
//!
//! ```compile_fail,E0308
//! use core::ops::Deref;
//! use tailq::Link;
//! static REAL: Link = Link::new();
//! struct Wrapper;
//! impl Deref for Wrapper {
//!     type Target = Link;
//!     fn deref(&self) -> &Link { &REAL }
//! }
//! struct Node { link: Wrapper }
//! tailq::adapter!(NodeLink = Node { link });
//! ```
//!
//! A nested path through a box:
//!
//! ```compile_fail,E0609
//! use tailq::Link;
//! struct Sched { link: Link }
//! struct Node { sched: Box<Sched> }
//! tailq::adapter!(NodeLink = Node { sched.link });
//! ```
//!
//! A payload of another type than its field's, or one inside a packed
//! field, which may leave it unaligned while the link stays aligned:
//!
//! ```compile_fail,E0308
//! use tailq::Link;
//! struct Node { link: Link, key: u32 }
//! tailq::adapter!(NodeLink = Node { link, mut key: u64 });
//! ```
//!
//! ```compile_fail,E0080
//! use tailq::Link;
//! #[repr(C, packed)]
//! struct Packed { tag: u8, key: u32 }
//! struct Node { link: Link, packed: Packed }
//! tailq::adapter!(NodeLink = Node { link, mut packed.key: u32 });
//! ```
//!
//! The macro's own rules, called directly, cannot lend the link as a
//! payload: the queue checks the payload against the adapter's real link.
//!
//! ```compile_fail,E0080
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! tailq::__adapter!(@payload NodeLink [] [] [] Node, [link: Link]);
//! let mut node = Node { link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! let _ = queue.front_mut();
//! ```
//!
//! A brace group in the bounds cannot close the adapter's header and take
//! the place of its body:
//!
//! ```compile_fail
//! use tailq::Link;
//! struct Node { link: Link }
//! struct Dummy;
//! trait Sink { type Node; const OFFSET: usize; }
//! tailq::adapter!(NodeLink = Node { link } where Node: Sized {
//!     type Node = Node;
//!     const OFFSET: usize = 999;
//! } impl Sink for Dummy);
//! ```
//!
//! Only `adapter!` implements `Adapter`: the trait is sealed.
//!
//! ```compile_fail,E0277
//! use tailq::{Adapter, Link};
//! struct Node { link: Link }
//! enum ByHand {}
//! unsafe impl Adapter for ByHand {
//!     type Node = Node;
//!     const OFFSET: usize = 0;
//! }
//! ```
//!
//! A node that leaves is handed back, and dropping it unused is a warning:
//!
//! ```compile_fail
//! #![deny(unused_must_use)]
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link }
//! tailq::adapter!(NodeLink = Node { link });
//! let mut node = Node { link: Link::new() };
//! let mut queue = TailQueue::<NodeLink>::new();
//! queue.push_back(&mut node);
//! queue.cursor_front_mut().remove_current();
//! ```
