// Copyright (c) 2024 Linaro LTD
// SPDX-License-Identifier: Apache-2.0

//! # Zephyr low-level synchronization primities.
//!
//! The `zephyr-sys` crate contains direct calls into the Zephyr C API.  This interface, however,
//! cannot be used from safe Rust.  This crate attempts to be as direct an interface to some of
//! these synchronization mechanisms, but without the need for unsafe.  The other module
//! `crate::sync` provides higher level interfaces that help manage synchronization in coordination
//! with Rust's borrowing and sharing rules, and will generally provide much more usable
//! interfaces.
//!
//! # Kernel objects
//!
//! Zephyr's primitives work with the concept of a kernel object.  These are the data structures
//! that are used by the Zephyr kernel to coordinate the operation of the primitives.  In addition,
//! they are where the protection barrier provided by `CONFIG_USERSPACE` is implemented.  In order
//! to use these primitives from a userspace thread two things must happen:
//!
//! - The kernel objects must be specially declared.  All kernel objects in Zephyr will be built,
//!   at compile time, into a perfect hash table that is used to validate them.  The special
//!   declaration will take care of this.
//! - The objects must be granted permission to be used by the userspace thread.  This can be
//!   managed either by specifically granting permission, or by using inheritance when creating the
//!   thread.
//!
//! At this time, only the first mechanism is implemented, and all kernel objects should be
//! declared using the `crate::kobj_define!` macro.  These then must be initialized, and then the
//! special method `.get()` called, to retrieve the Rust-style value that is used to manage them.
//! Later, there will be a pool mechanism to allow these kernel objects to be allocated and freed
//! from a pool, although the objects will still be statically allocated.

use core::fmt;

use crate::error::{Result, to_result_void};
use crate::raw::{
    k_condvar,
    k_condvar_init,
    k_condvar_broadcast,
    k_condvar_signal,
    k_condvar_wait,
    k_mutex,
    k_mutex_init,
    k_mutex_lock,
    k_mutex_unlock,
};
use crate::object::{
    KobjInit,
    StaticKernelObject,
};
use crate::time::{
    Timeout,
};
use super::K_FOREVER;

/// A Zephyr `k_mutux` usable from safe Rust code.
///
/// This merely wraps a pointer to the kernel object.  It implements clone, send and sync as it is
/// safe to have multiple instances of these, as well as use them across multiple threads.
///
/// Note that these are Safe in the sense that memory safety is guaranteed.  Attempts to
/// recursively lock, or incorrect nesting can easily result in deadlock.
#[derive(Clone)]
pub struct Mutex {
    /// The raw Zephyr mutex.
    item: *mut k_mutex,
}

unsafe impl Sync for StaticKernelObject<k_mutex> {}

impl KobjInit<k_mutex, Mutex> for StaticKernelObject<k_mutex> {
    fn wrap(ptr: *mut k_mutex) -> Mutex {
        Mutex { item: ptr }
    }
}

impl Mutex {
    /// Lock a Zephyr Mutex.
    ///
    /// Will wait for the lock, returning status, with `Ok(())` indicating the lock has been
    /// acquired, and an error indicating a timeout (Zephyr returns different errors depending on
    /// the reason).
    pub fn lock<T>(&self, timeout: T) -> Result<()>
        where T: Into<Timeout>,
    {
        let timeout: Timeout = timeout.into();
        to_result_void(unsafe { k_mutex_lock(self.item, timeout.0) })
    }

    /// Unlock a Zephyr Mutex.
    ///
    /// The mutex must already be locked by the calling thread.  Mutexes may not be unlocked in
    /// ISRs.
    pub fn unlock(&self) -> Result<()> {
        to_result_void(unsafe { k_mutex_unlock(self.item) })
    }
}

unsafe impl Sync for Mutex {}
unsafe impl Send for Mutex {}

impl fmt::Debug for Mutex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sys::Mutex {:?}", self.item)
    }
}

/// A static Zephyr `k_mutex`.
///
/// This is intended to be used from within the `kobj_define!` macro.  It declares a static mutex
/// that will be properly registered with the Zephyr kernel object system.  The `init` method must
/// be called before `get`.
///
/// ```
/// kobj_define! {
///     static SINGLE: StaticMutex;
///     static MULTIPLE: [StaticMutex; 4];
/// }
///
/// let multiple  MULTIPLE.each_ref().map(|m| {
///     m.init();
///     m.get()
/// });
///
/// SINGLE.init();
/// let single = SINGLE.get();
/// ...
/// ```
pub type StaticMutex = StaticKernelObject<k_mutex>;

impl StaticMutex {
    /// Initialize the Zephyr mutex.
    ///
    /// Must be called before calling [`get`].
    ///
    /// [`get`]: KobjInit::get
    pub fn init(&self) {
        self.init_help(|raw| {
            unsafe {
                // Init is defined to always return zero, no error possible.
                k_mutex_init(raw);
            }
        })
    }
}

/// A Condition Variable
///
/// Lightweight wrappers for Zephyr's `k_condvar`.
#[derive(Clone)]
pub struct Condvar {
    /// The underlying `k_condvar`.
    item: *mut k_condvar,
}

unsafe impl Sync for StaticKernelObject<k_condvar> { }

unsafe impl Sync for Condvar {}
unsafe impl Send for Condvar {}

impl KobjInit<k_condvar, Condvar> for StaticKernelObject<k_condvar> {
    fn wrap(ptr: *mut k_condvar) -> Condvar {
        Condvar { item: ptr }
    }
}

/// A static Zephyr `k_condvar` wrapped with a Rust kernel object manager.
///
/// This should only be declared inside of the `kobj_define!` macro.
pub type StaticCondvar = StaticKernelObject<k_condvar>;

impl StaticCondvar {
    /// Initialize the underlying Zephyr condvar.
    ///
    /// Must be called before calling [`get`].
    ///
    /// [`get`]: KobjInit::get
    pub fn init(&self) {
        self.init_help(|raw| {
            unsafe {
                k_condvar_init(raw);
            }
        })
    }
}

impl Condvar {
    /// Wait for someone else using this mutex/condvar pair to notify.
    ///
    /// Note that this requires the lock to be held by use, but as this is a low-level binding to
    /// Zephyr's interfaces, this is not enforced.  See [`sync::Condvar`] for a safer and easier to
    /// use interface.
    ///
    /// [`sync::Condvar`]: crate::sync::Condvar
    pub fn wait(&self, lock: &Mutex) {
        unsafe { k_condvar_wait(self.item, lock.item, K_FOREVER); }
    }

    // TODO: timeout.

    /// Wake a single thread waiting on this condition variable.
    pub fn notify_one(&self) {
        unsafe { k_condvar_signal(self.item); }
    }

    /// Wake all threads waiting on this condition variable.
    pub fn notify_all(&self) {
        unsafe { k_condvar_broadcast(self.item); }
    }
}

impl fmt::Debug for Condvar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sys::Condvar {:?}", self.item)
    }
}
