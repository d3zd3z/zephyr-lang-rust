//! Lightweight wrapper around Zephyr's `k_queue`.
//!
//! The underlying operations on the `k_queue` are all unsafe, as the model does not match the
//! borrowing model that Rust expects.  This module is mainly intended to be used by the
//! implementation of `zephyr::sys::channel`, which can be used without needing unsafe.

use core::ffi::c_void;

use zephyr_sys::{
    k_queue,
    k_queue_init,
    k_queue_append,
    k_queue_get,
};

use crate::sys::K_FOREVER;
use crate::object::{KobjInit, StaticKernelObject};

/// A wrapper around a Zephyr `k_queue` object.
#[derive(Clone, Debug)]
pub struct Queue {
    item: *mut k_queue,
}

unsafe impl Sync for StaticKernelObject<k_queue> { }

unsafe impl Sync for Queue { }
unsafe impl Send for Queue { }

impl Queue {
    /// Append an element to the end of a queue.
    ///
    /// This adds an element to the given [`Queue`].  Zephyr requires the
    /// first word of this message to be available for the OS to enqueue
    /// the message.  See [`Message`] for details on how this can be used
    /// safely.
    ///
    /// [`Message`]: crate::sync::channel::Message
    pub unsafe fn send(&self, data: *mut c_void) {
        k_queue_append(self.item, data)
    }

    /// Get an element from a queue.
    ///
    /// This routine removes the first data item from the [`Queue`].
    pub unsafe fn recv(&self) -> *mut c_void {
        k_queue_get(self.item, K_FOREVER)
    }
}

impl KobjInit<k_queue, Queue> for StaticKernelObject<k_queue> {
    fn wrap(ptr: *mut k_queue) -> Queue {
        Queue { item: ptr }
    }
}

/// A statically defined Zephyr `k_queue`.
///
/// This should be declared as follows:
/// ```
/// kobj_define! {
///     static MY_QUEUE: StaticQueue;
/// }
///
/// MY_QUEUE.init();
/// let my_queue = MY_QUEUE.get();
///
/// my_queue.send(...);
/// ```
pub type StaticQueue = StaticKernelObject<k_queue>;

impl StaticQueue {
    /// Initialize the underlying Zephyr `k_queue`.
    ///
    /// Must be called before calling [`get`].
    ///
    /// [`get`]: KobjInit::get
    pub fn init(&self) {
        self.init_help(|raw| {
            unsafe {
                k_queue_init(raw);
            }
        })
    }
}
