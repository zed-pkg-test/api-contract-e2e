//! Checked-handle runtime foundation for a single owning actor/arena domain.
//!
//! This is executable Rust **host/runtime infrastructure**, not a guest-Wasm
//! lowering or a general Pointer<T> ABI. The guest compiler must keep
//! rejecting ref!/deref!/unref! until typed MIR independently proves all-path
//! ownership, cancellation/exception cleanup, domain authority and effects.
//!
//! A checked handle is an opaque (arena, actor, domain, slot, generation)
//! capability. ref_owned consumes T; deref_checked lends an immutable view;
//! unref_owned restores T exactly once, only after *all* views have ended.
//! No raw address or mutable access is exposed. The registry is bounded,
//! synchronized and generation-checked; stale handles cannot exploit slot
//! reuse, and overflow permanently retires an exhausted slot.
//!
//! The actor/domain IDs are trusted runtime inputs, NOT independently
//! authenticated caller credentials. The eventual actor scheduler must hold
//! the arena behind a non-forgeable domain capability.

use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const MAX_SLOTS: usize = 16_384;
static NEXT_ARENA_ID: AtomicU64 = AtomicU64::new(1);

/// Every result is explicit. In particular, stale or concurrently borrowed
/// handles never silently convert to an owned value or raw host pointer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandleError {
    Capacity,
    ArenaIdExhausted,
    ArenaMismatch,
    ActorMismatch,
    DomainMismatch,
    InvalidSlot,
    StaleGeneration,
    ActiveViews,
    Poisoned,
}

/// A failed ref_owned admission must return the original owner instead
/// of dropping it invisibly. This is separate from errors after issuance.
pub struct RefAdmissionError<T> {
    pub reason: HandleError,
    pub owned: T,
}

impl<T> std::fmt::Debug for RefAdmissionError<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("RefAdmissionError")
            .field("reason", &self.reason)
            .field("owned", &"<retained>")
            .finish()
    }
}

/// Typed, opaque checked-handle identity. Copying it does **not** copy or
/// create ownership; the generation ledger rejects a duplicate unref_owned.
#[derive(Debug)]
pub struct CheckedHandle<T> {
    arena: u64,
    actor: u64,
    domain: u64,
    slot: u32,
    generation: u32,
    _type: PhantomData<fn() -> T>,
}

impl<T> Copy for CheckedHandle<T> {}
impl<T> Clone for CheckedHandle<T> {
    fn clone(&self) -> Self { *self }
}

impl<T> CheckedHandle<T> {
    pub fn generation(&self) -> u32 { self.generation }
    pub fn slot(&self) -> u32 { self.slot }
}

struct Entry<T> {
    generation: u32,
    value: Option<Arc<T>>,
    /// When a generation reaches u32::MAX we never reuse the slot, which
    /// prevents ABA by wrapping generation back to 0.
    exhausted: bool,
}

struct Ledger<T> {
    entries: Vec<Entry<T>>,
}

/// One immutable view; its lifetime is tied to the owning arena reference.
/// Holding a view prevents unref_owned even though the arena is accessed
/// through &self and lock-based interior mutability.
pub struct CheckedRead<'a, T> {
    value: Arc<T>,
    _arena: PhantomData<&'a CheckedArena<T>>,
}

impl<T> Deref for CheckedRead<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target { &self.value }
}

/// Separate registry per actor-owned arena. The exact guest handle encoding
/// and actor-domain entry authorization are intentionally **not** provided.
pub struct CheckedArena<T> {
    actor: u64,
    domain: u64,
    id: u64,
    capacity: usize,
    ledger: Mutex<Ledger<T>>,
}

impl<T> CheckedArena<T> {
    pub fn new(actor: u64, domain: u64, capacity: usize) -> Result<Self, HandleError> {
        if capacity == 0 || capacity > MAX_SLOTS {
            return Err(HandleError::Capacity);
        }
        // Prevent ID wraparound: an exhausted identity must not alias a
        // previous arena, even if user code creates many short-lived arenas.
        let id = NEXT_ARENA_ID.fetch_update(
            Ordering::SeqCst, Ordering::SeqCst, |cur| cur.checked_add(1)
        ).map_err(|_| HandleError::ArenaIdExhausted)?;
        Ok(Self { actor, domain, id, capacity,
            ledger: Mutex::new(Ledger { entries: Vec::new() }) })
    }

    fn validate(&self, handle: CheckedHandle<T>, count: usize) -> Result<usize, HandleError> {
        if handle.arena != self.id { return Err(HandleError::ArenaMismatch); }
        if handle.actor != self.actor { return Err(HandleError::ActorMismatch); }
        if handle.domain != self.domain { return Err(HandleError::DomainMismatch); }
        let i = handle.slot as usize;
        if i >= count { return Err(HandleError::InvalidSlot); }
        Ok(i)
    }

    /// Checked equivalent of a future `ref! owner`: consume the owned value
    /// and install it in this arena, with a bounded typed slot identity.
    pub fn ref_owned(&self, value: T) -> Result<CheckedHandle<T>, RefAdmissionError<T>> {
        let mut state = match self.ledger.lock() {
            Ok(guard) => guard,
            Err(_) => return Err(RefAdmissionError {
                reason: HandleError::Poisoned, owned: value
            }),
        };
        let index = if let Some(i) = state.entries.iter().position(|s|
            s.value.is_none() && !s.exhausted
        ) {
            i
        } else {
            if state.entries.len() >= self.capacity {
                return Err(RefAdmissionError {
                    reason: HandleError::Capacity, owned: value
                });
            }
            state.entries.push(Entry {
                generation: 1, value: None, exhausted: false,
            });
            state.entries.len() - 1
        };
        let entry = &mut state.entries[index];
        debug_assert!(entry.value.is_none() && !entry.exhausted);
        entry.value = Some(Arc::new(value));
        Ok(CheckedHandle {
            arena: self.id, actor: self.actor, domain: self.domain,
            slot: index as u32, generation: entry.generation,
            _type: PhantomData,
        })
    }

    /// Checked equivalent of a read-only `deref! handle`. No owned value or
    /// raw pointer escapes, and a held view blocks unref_owned.
    pub fn deref_checked(&self, handle: CheckedHandle<T>) ->
        Result<CheckedRead<'_, T>, HandleError>
    {
        let state = self.ledger.lock().map_err(|_| HandleError::Poisoned)?;
        let i = self.validate(handle, state.entries.len())?;
        let entry = &state.entries[i];
        if handle.generation != entry.generation {
            return Err(HandleError::StaleGeneration);
        }
        let value = entry.value.as_ref().ok_or(HandleError::StaleGeneration)?;
        Ok(CheckedRead {
            value: Arc::clone(value), _arena: PhantomData,
        })
    }

    /// Checked equivalent of `unref! handle`, **not** free()/refcount--:
    /// transfer the original T back to the caller only when no dependent
    /// CheckedRead remains. The copied handle is invalidated by generation.
    pub fn unref_owned(&self, handle: CheckedHandle<T>) -> Result<T, HandleError> {
        let mut state = self.ledger.lock().map_err(|_| HandleError::Poisoned)?;
        let i = self.validate(handle, state.entries.len())?;
        let entry = &mut state.entries[i];
        if handle.generation != entry.generation {
            return Err(HandleError::StaleGeneration);
        }
        let Some(value) = entry.value.take() else {
            return Err(HandleError::StaleGeneration);
        };
        match Arc::try_unwrap(value) {
            Ok(owned) => {
                // u32::MAX is a one-shot final generation. Once consumed,
                // the slot becomes permanently exhausted.
                match entry.generation.checked_add(1) {
                    Some(next) => entry.generation = next,
                    None => entry.exhausted = true,
                }
                Ok(owned)
            }
            Err(value) => {
                entry.value = Some(value);
                Err(HandleError::ActiveViews)
            }
        }
    }

    pub fn live_handles(&self) -> Result<usize, HandleError> {
        let state = self.ledger.lock().map_err(|_| HandleError::Poisoned)?;
        Ok(state.entries.iter().filter(|e| e.value.is_some()).count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    #[test]
    fn roundtrip_restores_same_owned_value_without_cloning() {
        let arena = CheckedArena::new(3, 7, 4).unwrap();
        let owned = Box::new(String::from("hello"));
        let ptr = (&*owned) as *const String;
        let h = arena.ref_owned(owned).unwrap();
        let view = arena.deref_checked(h).unwrap();
        assert_eq!(view.as_str(), "hello");
        drop(view);
        let back = arena.unref_owned(h).unwrap();
        assert_eq!((&*back) as *const String, ptr);
        assert_eq!(back.as_str(), "hello");
        assert_eq!(arena.live_handles(), Ok(0));
    }

    #[test]
    fn multiple_views_prevent_retirement_until_last_scope_exits() {
        let arena = CheckedArena::new(1, 1, 3).unwrap();
        let h = arena.ref_owned(vec![1, 2, 3]).unwrap();
        let a = arena.deref_checked(h).unwrap();
        let b = arena.deref_checked(h).unwrap();
        assert_eq!(a.len(), 3);
        assert_eq!(arena.unref_owned(h), Err(HandleError::ActiveViews));
        drop(a);
        assert_eq!(arena.unref_owned(h), Err(HandleError::ActiveViews));
        drop(b);
        assert_eq!(arena.unref_owned(h), Ok(vec![1, 2, 3]));
    }

    #[test]
    fn double_unref_and_deref_after_unref_are_rejected() {
        let arena = CheckedArena::new(1, 1, 2).unwrap();
        let h = arena.ref_owned(42).unwrap();
        let duplicate = h;
        assert_eq!(arena.unref_owned(h), Ok(42));
        assert_eq!(arena.unref_owned(duplicate), Err(HandleError::StaleGeneration));
        assert!(matches!(arena.deref_checked(duplicate), Err(HandleError::StaleGeneration)));
    }

    #[test]
    fn slot_reuse_increments_generation_and_rejects_stale_copy() {
        let arena = CheckedArena::new(1, 1, 1).unwrap();
        let old = arena.ref_owned(41).unwrap();
        assert_eq!(arena.unref_owned(old), Ok(41));
        let new = arena.ref_owned(42).unwrap();
        assert_eq!(new.slot(), old.slot());
        assert_eq!(new.generation(), old.generation() + 1);
        assert!(matches!(arena.deref_checked(old), Err(HandleError::StaleGeneration)));
        assert_eq!(arena.unref_owned(new), Ok(42));
    }

    #[test]
    fn foreign_arena_is_rejected_even_when_actor_domain_ids_equal() {
        let a: CheckedArena<i32> = CheckedArena::new(1, 1, 2).unwrap();
        let b: CheckedArena<i32> = CheckedArena::new(1, 1, 2).unwrap();
        let h = a.ref_owned(42).unwrap();
        assert_eq!(b.unref_owned(h), Err(HandleError::ArenaMismatch));
        assert!(matches!(b.deref_checked(h), Err(HandleError::ArenaMismatch)));
        assert_eq!(a.unref_owned(h), Ok(42));
    }

    #[test]
    fn corrupted_actor_and_domain_fields_are_rejected() {
        let arena = CheckedArena::new(9, 8, 2).unwrap();
        let h = arena.ref_owned(42).unwrap();
        let mut bad_actor = h;
        bad_actor.actor = 10;
        assert_eq!(arena.unref_owned(bad_actor), Err(HandleError::ActorMismatch));
        let mut bad_domain = h;
        bad_domain.domain = 11;
        assert!(matches!(arena.deref_checked(bad_domain), Err(HandleError::DomainMismatch)));
        assert_eq!(arena.unref_owned(h), Ok(42));
    }

    #[test]
    fn capacity_is_bounded_and_live_slots_are_not_recycled() {
        assert!(matches!(CheckedArena::<u8>::new(1, 1, 0), Err(HandleError::Capacity)));
        assert!(matches!(CheckedArena::<u8>::new(1, 1, MAX_SLOTS+1), Err(HandleError::Capacity)));
        let arena = CheckedArena::new(1, 1, 1).unwrap();
        let h = arena.ref_owned(42).unwrap();
        let rejected = arena.ref_owned(33).err().expect("capacity rejection");
        assert_eq!(rejected.reason, HandleError::Capacity);
        assert_eq!(rejected.owned, 33);
        assert_eq!(arena.unref_owned(h), Ok(42));
        let h2 = arena.ref_owned(33).unwrap();
        assert_eq!(arena.unref_owned(h2), Ok(33));
    }

    #[test]
    fn generation_overflow_never_reuses_old_slot() {
        let arena = CheckedArena::new(1, 1, 1).unwrap();
        let mut h = arena.ref_owned(42).unwrap();
        {
            let mut state = arena.ledger.lock().unwrap();
            state.entries[0].generation = u32::MAX;
            h.generation = u32::MAX;
        }
        assert_eq!(arena.unref_owned(h), Ok(42));
        let rejected = arena.ref_owned(13).err().expect("exhausted slot");
        assert_eq!(rejected.reason, HandleError::Capacity);
        assert_eq!(rejected.owned, 13);
        assert!(matches!(arena.deref_checked(h), Err(HandleError::StaleGeneration)));
    }

    #[test]
    fn failed_admission_retains_original_owner_without_drop() {
        struct Tracked(Arc<AtomicUsize>);
        impl Drop for Tracked {
            fn drop(&mut self) {
                self.0.fetch_add(1, AtomicOrdering::SeqCst);
            }
        }
        let count = Arc::new(AtomicUsize::new(0));
        let arena = CheckedArena::new(1, 1, 1).unwrap();
        let h = arena.ref_owned(Tracked(Arc::clone(&count))).ok().unwrap();
        let rejected = arena.ref_owned(Tracked(Arc::clone(&count))).err().unwrap();
        assert_eq!(rejected.reason, HandleError::Capacity);
        assert_eq!(count.load(AtomicOrdering::SeqCst), 0);
        drop(rejected.owned);
        assert_eq!(count.load(AtomicOrdering::SeqCst), 1);
        let owned = arena.unref_owned(h).unwrap();
        drop(owned);
        assert_eq!(count.load(AtomicOrdering::SeqCst), 2);
    }

    #[test]
    fn racing_unref_is_linearizable_and_one_owner_survives() {
        use std::sync::Barrier;
        use std::thread;
        let arena = Arc::new(CheckedArena::new(1, 1, 1).unwrap());
        let handle = arena.ref_owned(42u64).ok().unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let workers: Vec<_> = (0..2).map(|_| {
            let arena = Arc::clone(&arena);
            let gate = Arc::clone(&barrier);
            thread::spawn(move || {
                gate.wait();
                arena.unref_owned(handle)
            })
        }).collect();
        barrier.wait();
        let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|r| **r == Ok(42)).count(), 1);
        assert_eq!(results.iter().filter(|r| **r == Err(HandleError::StaleGeneration)).count(), 1);
        assert_eq!(arena.live_handles(), Ok(0));
    }

    #[test]
    fn retired_values_are_dropped_exactly_once() {
        struct DropCount(Arc<AtomicUsize>);
        impl Drop for DropCount {
            fn drop(&mut self) { self.0.fetch_add(1, AtomicOrdering::SeqCst); }
        }
        let counter = Arc::new(AtomicUsize::new(0));
        let arena = CheckedArena::new(1, 1, 2).unwrap();
        let h = arena.ref_owned(DropCount(Arc::clone(&counter))).unwrap();
        let owned = arena.unref_owned(h).unwrap();
        assert_eq!(counter.load(AtomicOrdering::SeqCst), 0);
        drop(owned);
        assert_eq!(counter.load(AtomicOrdering::SeqCst), 1);
        drop(arena);
        assert_eq!(counter.load(AtomicOrdering::SeqCst), 1);
    }
}
