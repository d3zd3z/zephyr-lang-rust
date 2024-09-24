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

#[derive(Clone, Debug)]
pub struct Queue {
    pub item: *mut k_queue,
}

unsafe impl Sync for StaticKernelObject<k_queue> { }

unsafe impl Sync for Queue { }
unsafe impl Send for Queue { }

impl Queue {
    pub unsafe fn send(&self, data: *mut c_void) {
        k_queue_append(self.item, data)
    }

    pub unsafe fn recv(&self) -> *mut c_void {
        k_queue_get(self.item, K_FOREVER)
    }
}

impl KobjInit<k_queue, Queue> for StaticKernelObject<k_queue> {
    fn wrap(ptr: *mut k_queue) -> Queue {
        Queue { item: ptr }
    }
}

pub type StaticQueue = StaticKernelObject<k_queue>;

impl StaticQueue {
    pub fn init(&self) {
        self.init_help(|raw| {
            unsafe {
                k_queue_init(raw);
            }
        })
    }
}
