// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! Callers on older editions, whose identifiers a later edition reserves.
//!
//! The eighth review found that an adapter with a payload, named `gen` by
//! a 2021 caller, failed to compile, since the expansion passed its name
//! as a generic argument, which parsed it as a const expression:
//!
//! ```edition2021
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link, key: u32 }
//! tailq::adapter!(gen = Node { link, mut key: u32 });
//! fn main() {
//!     let mut node = Node { link: Link::new(), key: 1 };
//!     let mut queue = TailQueue::<gen>::new();
//!     queue.push_back(&mut node);
//!     assert_eq!(queue.front_mut().map(|key| *key), Some(1));
//! }
//! ```
//!
//! and so did one named `await` by a 2015 caller:
//!
//! ```edition2015
//! extern crate tailq;
//! use tailq::{Link, TailQueue};
//! struct Node { link: Link, key: u32 }
//! tailq::adapter!(await = Node { link, mut key: u32 });
//! fn main() {
//!     let mut node = Node { link: Link::new(), key: 1 };
//!     let mut queue = TailQueue::<await>::new();
//!     queue.push_back(&mut node);
//!     assert_eq!(queue.front_mut().map(|key| *key), Some(1));
//! }
//! ```
