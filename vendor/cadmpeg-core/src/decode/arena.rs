// SPDX-License-Identifier: Apache-2.0
//! Append-only byte store with stable slice addresses.

use std::cell::RefCell;
use std::ptr::NonNull;

use super::context::DecodeContext;
use crate::CodecError;

/// Owns stable byte buffers allocated during a decode.
#[derive(Debug, Default)]
pub struct DecodeArena {
    buffers: RefCell<Vec<OwnedBuffer>>,
}

impl DecodeArena {
    /// Creates an empty arena.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `bytes` and returns a borrow valid for the arena's lifetime.
    ///
    /// The returned slice is stable: later `alloc` calls never invalidate it.
    pub fn alloc(&self, ctx: &DecodeContext<'_>, bytes: Box<[u8]>) -> Result<&[u8], CodecError> {
        let mut buffers = self.buffers.borrow_mut();
        ctx.reserve_vec(&mut buffers, 1, "arena registry")?;
        let buffer = OwnedBuffer(NonNull::from(Box::leak(bytes)));
        let pointer = buffer.0;
        buffers.push(buffer);
        // SAFETY: the arena owns every buffer until it is dropped, and no
        // method mutates a buffer. The borrow cannot outlive the arena.
        Ok(unsafe { pointer.as_ref() })
    }
}

/// Owns one allocation without imposing a movable Box's unique-borrow tag.
#[derive(Debug)]
struct OwnedBuffer(NonNull<[u8]>);

// SAFETY: the allocation is uniquely owned, contains only bytes, and has no
// mutation API. Shared arena borrows cannot cross threads because RefCell
// makes DecodeArena !Sync.
unsafe impl Send for OwnedBuffer {}

impl Drop for OwnedBuffer {
    fn drop(&mut self) {
        // SAFETY: this pointer came from exactly one Box::leak and is released
        // exactly once. DecodeArena owns it and outlives every borrowed slice.
        unsafe { drop(Box::from_raw(self.0.as_ptr())) };
    }
}

#[cfg(test)]
mod tests {
    use super::DecodeArena;
    use crate::decode::{DecodeContext, DecodePolicy};

    fn buffer(index: usize) -> Box<[u8]> {
        let len = (index % 7) + 1;
        vec![u8::try_from(index).expect("test index fits u8"); len].into_boxed_slice()
    }

    fn check(index: usize, slice: &[u8]) {
        assert_eq!(slice.len(), (index % 7) + 1, "length of buffer {index}");
        assert!(
            slice
                .iter()
                .all(|&byte| byte == u8::try_from(index).expect("test index fits u8")),
            "contents of buffer {index}",
        );
    }

    #[test]
    fn empty_buffer_borrows_survive_later_allocations() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
            .expect("empty root");
        let empty = arena
            .alloc(&ctx, Vec::new().into_boxed_slice())
            .expect("admitted registry");
        let present = arena
            .alloc(&ctx, vec![7].into_boxed_slice())
            .expect("admitted registry");
        assert!(empty.is_empty());
        assert_eq!(present, &[7]);
    }

    #[test]
    fn an_owned_arena_can_move_between_threads() {
        let arena = DecodeArena::new();
        {
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
                .expect("empty root");
            arena
                .alloc(&ctx, vec![7].into_boxed_slice())
                .expect("admitted registry");
        }
        let arena = std::thread::spawn(move || {
            {
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
                        .expect("empty root");
                assert_eq!(
                    arena
                        .alloc(&ctx, vec![9].into_boxed_slice())
                        .expect("admitted registry"),
                    &[9]
                );
            }
            arena
        })
        .join()
        .expect("the receiving thread retains and reads the arena");
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
            .expect("empty root");
        assert_eq!(
            arena
                .alloc(&ctx, vec![11].into_boxed_slice())
                .expect("admitted registry"),
            &[11]
        );
    }

    /// Alloc while earlier borrows stay live: each iteration allocates, then
    /// re-reads every prior slice.
    #[test]
    fn interleaved_alloc_and_read_across_many_buffers() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
            .expect("empty root");
        let mut borrows: Vec<&[u8]> = Vec::new();
        for index in 0..256usize {
            borrows.push(arena.alloc(&ctx, buffer(index)).expect("admitted registry"));
            for (i, slice) in borrows.iter().enumerate() {
                check(i, slice);
            }
        }
    }
}
