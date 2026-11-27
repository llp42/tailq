// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Leonardo Lopes Pereira <leonardolopespereira@outlook.com>

//! `flip!(key; T0, T1, ...)` expands to `T0` the first time it is expanded
//! with that key, `T1` the second, and so on, then the last one: a type
//! macro that is not a function of its input, written in safe code.  With
//! it, a test can tell whether `adapter!` pastes a type more than once,
//! since each paste could then name another type.
//!
//! `tailq`'s tests that need it live here, so that `tailq` has no
//! dependency on it, which its package could not ship: `tests/flip.rs`
//! checks that every adapter measures the node it declares, and the
//! doctests below what must not compile.
//!
//! A type macro expands at each paste of a type into the expansion, so a
//! macro answering differently each time could lend a 64-byte payload
//! over a one-byte field if the payload type were pasted twice.  Each type
//! is pasted once, and a payload whose one paste is not its field's type
//! fails:
//!
//! ```compile_fail,E0308
//! use tailq::Link;
//! struct Node { link: Link, key: u8 }
//! tailq::adapter!(NodeLink = Node { link, mut key: tailq_flip::flip!(k; [u8; 64], u8) });
//! ```
//!
//! The raw place proving the payload's type names it as `Self::Payload`,
//! not by a second paste.  A paste that answered `Box<u64>` there, after
//! `u64` for the payload type, would pass the reference check too, a
//! coercion site where the box derefs to its `u64`, and lend the box's
//! pointer as a `&mut u64`:
//!
//! ```compile_fail,E0308
//! use tailq::Link;
//! struct Node { link: Link, key: Box<u64> }
//! tailq::adapter!(NodeLink = Node { link, mut key: tailq_flip::flip!(boxed; u64, Box<u64>) });
//! ```

#![forbid(unsafe_code)]

use core::mem::take;
use proc_macro::{TokenStream, TokenTree};
use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

/// How many times each key has been expanded.
static COUNTS: Mutex<Option<HashMap<String, usize>>> = Mutex::new(None);

/// Returns how many times `key` was expanded before, and counts this one.
fn expansions(key: String) -> usize {
    let mut guard = COUNTS.lock().unwrap_or_else(PoisonError::into_inner);
    let count = guard
        .get_or_insert_with(HashMap::new)
        .entry(key)
        .or_insert(0);
    let before = *count;
    *count = count.saturating_add(1);
    drop(guard);
    before
}

/// Expands to the next of its types for its key.
#[proc_macro]
#[inline]
pub fn flip(input: TokenStream) -> TokenStream {
    let mut tokens = input.into_iter();
    let key = tokens
        .next()
        .map(|token| token.to_string())
        .unwrap_or_default();
    let mut choices: Vec<TokenStream> = Vec::new();
    let mut choice: Vec<TokenTree> = Vec::new();
    let mut depth: usize = 0;
    for token in tokens.skip(1) {
        if let TokenTree::Punct(punct) = token.clone() {
            match punct.as_char() {
                '<' => depth = depth.saturating_add(1),
                '>' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    choices.push(take(&mut choice).into_iter().collect());
                    continue;
                }
                _ => {}
            }
        }
        choice.push(token);
    }
    if !choice.is_empty() {
        choices.push(choice.into_iter().collect());
    }
    let index = expansions(key).min(choices.len().saturating_sub(1));
    choices.into_iter().nth(index).unwrap_or_default()
}
