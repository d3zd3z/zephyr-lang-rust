//! Higher level synchronization primitives.
//!
//! These are modeled after the synchronization primitives in
//! [`std::sync`](https://doc.rust-lang.org/stable/std/sync/index.html) and those from
//! [`crossbeam-channel`](https://docs.rs/crossbeam-channel/latest/crossbeam_channel/), in as much
//! as it makes sense.

use core::{
    cell::UnsafeCell,
    fmt,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use crate::time::Forever;
use crate::sys::sync as sys;

pub mod atomic {
    //! Re-export portable atomic.
    //!
    //! Although `core` contains a
    //! [`sync::atomic`](https://doc.rust-lang.org/stable/core/sync/atomic/index.html) module,
    //! these are dependent on the target having atomic instructions, and the types are missing
    //! when the platform cannot support them.  Zephyr, however, does provide atomics on platforms
    //! that don't support atomic instructions, using spinlocks.  In the Rust-embedded world, this
    //! is done through the [`portable-atomic`](https://crates.io/crates/portable-atomic) crate,
    //! which will either just re-export the types from core, or provide an implementation using
    //! spinlocks when those aren't available.

    pub use portable_atomic::*;
}

#[cfg(CONFIG_RUST_ALLOC)]
pub use portable_atomic_util::Arc;

/// Until poisoning is implemented, mutexes never return an error, and we just get back the guard.
pub type LockResult<Guard> = Result<Guard, ()>;

/// A mutual exclusion primitive useful for protecting shared data.
///
/// This mutex will block threads waiting for the lock to become available. This is modeled after
/// [`std::sync::Mutex`](https://doc.rust-lang.org/stable/std/sync/struct.Mutex.html), and attempts
/// to implement that API as closely as makes sense on Zephyr.  Currently, it has the following
/// differences:
/// - Poisoning: This does not yet implement poisoning, as there is no way to recover from panic at
///   this time on Zephyr.
/// - Allocation: `new` is not yet provided, and will be provided once kernel object pools are
///   implemented.  Please use `new_from` which takes a reference to a statically allocated
///   `sys::Mutex`.
pub struct Mutex<T: ?Sized> {
    inner: sys::Mutex,
    // poison: ...
    data: UnsafeCell<T>,
}

// At least if correctly done, the Mutex provides for Send and Sync as long as the inner data
// supports Send.
unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

impl<T> fmt::Debug for Mutex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Mutex {:?}", self.inner)
    }
}

/// An RAII implementation of a "scoped lock" of a mutex.  When this structure is dropped (faslls
/// out of scope), the lock will be unlocked.
///
/// The data protected by the mutex can be accessed through this guard via its [`Deref`] and
/// [`DerefMut`] implementations.
///
/// This structure is created by the [`lock`] and [`try_lock`] methods on [`Mutex`].
///
/// Taken directly from
/// [`std::sync::MutexGuard`](https://doc.rust-lang.org/stable/std/sync/struct.MutexGuard.html).
pub struct MutexGuard<'a, T: ?Sized + 'a> {
    lock: &'a Mutex<T>,
    // until <https://github.com/rust-lang/rust/issues/68318> is implemented, we have to mark unsend
    // explicitly.  This can be done by holding Phantom data with an unsafe cell in it.
    _nosend: PhantomData<UnsafeCell<()>>,
}

// Make sure the guard doesn't get sent.
// Negative trait bounds are unstable, see marker above.
// impl<T: ?Sized> !Send for MutexGuard<'_, T> {}
unsafe impl<T: ?Sized + Sync> Sync for MutexGuard<'_, T> {}

impl<T> Mutex<T> {
    /// Construct a new wrapped Mutex, using the given underlying sys mutex.  This is different that
    /// `std::sync::Mutex` in that in Zephyr, objects are frequently allocated statically, and the
    /// sys Mutex will be taken by this structure.  It is safe to share the underlying Mutex between
    /// different items, but without careful use, it is easy to deadlock, so it is not recommended.
    pub const fn new_from(t: T, raw_mutex: sys::Mutex) -> Mutex<T> {
        Mutex { inner: raw_mutex, data: UnsafeCell::new(t) }
    }
}

impl<T: ?Sized> Mutex<T> {
    /// Acquires a mutex, blocking the current thread until it is able to do so.
    ///
    /// This function will block the local thread until it is available to acquire the mutex. Upon
    /// returning, the thread is the only thread with the lock held. An RAII guard is returned to
    /// allow scoped unlock of the lock. When the guard goes out of scope, the mutex will be
    /// unlocked.
    ///
    /// In `std`, an attempt to lock a mutex by a thread that already holds the mutex is
    /// unspecified.  Zephyr explicitly supports this behavior, by simply incrementing a lock
    /// count.
    pub fn lock(&self) -> LockResult<MutexGuard<'_, T>> {
        // With `Forever`, should never return an error.
        self.inner.lock(Forever).unwrap();
        unsafe {
            MutexGuard::new(self)
        }
    }
}

impl<'mutex, T: ?Sized> MutexGuard<'mutex, T> {
    unsafe fn new(lock: &'mutex Mutex<T>) -> LockResult<MutexGuard<'mutex, T>> {
        // poison todo
        Ok(MutexGuard { lock, _nosend: PhantomData })
    }
}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe {
            &*self.lock.data.get()
        }
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T: ?Sized> Drop for MutexGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        self.lock.inner.unlock().unwrap();
    }
}
