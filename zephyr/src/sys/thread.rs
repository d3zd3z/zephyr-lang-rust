//! Zephyr low level threads
//!
//! This is a fairly low level (but still safe) interface to Zephyr threads.  This is intended to
//! work the same way as threads are typically done on Zephyr systems, where the threads and their
//! stacks are statically allocated, a code is called to initialize them.
//!
//! In addition, there are some convenience operations available that require allocation to be
//! available.

use zephyr_sys::{
    k_thread, k_thread_create, k_thread_start, z_thread_stack_element, ZR_STACK_ALIGN, ZR_STACK_RESERVED
};

use core::{cell::UnsafeCell, ffi::c_void, ptr::null_mut};

use crate::{align::AlignAs, object::{KobjInit, StaticKernelObject}};

#[cfg(CONFIG_RUST_ALLOC)]
extern crate alloc;
#[cfg(CONFIG_RUST_ALLOC)]
use alloc::boxed::Box;
#[cfg(CONFIG_RUST_ALLOC)]
use core::mem::ManuallyDrop;

use super::K_FOREVER;

/// Adjust the stack size for alignment.  Note that, unlike the C code, we don't include the
/// reservation in this, as it has its own fields in the struct.
pub const fn stack_len(size: usize) -> usize {
    size.next_multiple_of(ZR_STACK_ALIGN)
}

/// A Zephyr stack declaration.  It isn't meant to be used directly, as it needs additional
/// decoration about linker sections and such.  Unlike the C declaration, the reservation is a
/// separate field.  As long as the SIZE is properly aligned, this should work without padding
/// between the fields.
pub struct ThreadStack<const SIZE: usize> {
    #[allow(dead_code)]
    align: AlignAs<ZR_STACK_ALIGN>,
    data: UnsafeCell<[z_thread_stack_element; SIZE]>,
    #[allow(dead_code)]
    extra: [z_thread_stack_element; ZR_STACK_RESERVED],
}

unsafe impl<const SIZE: usize> Sync for ThreadStack<SIZE> {}

impl<const SIZE: usize> ThreadStack<SIZE> {
    /// Get the size of this stack.  This is the size, minus any reservation.  This is called `size`
    /// to avoid any confusion with `len` which might return the actual size of the stack.
    pub fn size(&self) -> usize {
        SIZE
    }

    /// Return the stack base needed as the argument to various zephyr calls.
    pub fn base(&self) -> *mut z_thread_stack_element {
        self.data.get() as *mut z_thread_stack_element
    }

    /// Return the token information for this stack, which is a base and size.
    pub fn token(&self) -> StackToken {
        StackToken { base: self.base(), size: self.size() }
    }
}

/// Declare a variable, of a given name, representing the stack for a thread.
#[macro_export]
macro_rules! kernel_stack_define {
    ($name:ident, $size:expr) => {
        #[link_section = concat!(".noinit.", stringify!($name), ".", file!(), line!())]
        static $name: $crate::sys::thread::ThreadStack<{$crate::sys::thread::stack_len($size)}>
            = unsafe { ::core::mem::zeroed() };
    };
}

/// A single Zephyr thread.
///
/// This wraps a `k_thread` type within Zephyr.  This value is returned
/// from the `StaticThread::spawn` method, to allow control over the start
/// of the thread.  The [`start`] method should be used to start the
/// thread.
///
/// [`start`]: Thread::start
pub struct Thread {
    raw: *mut k_thread,
}

unsafe impl Sync for StaticKernelObject<k_thread> { }

impl KobjInit<k_thread, Thread> for StaticKernelObject<k_thread> {
    fn wrap(ptr: *mut k_thread) -> Thread {
        Thread { raw: ptr }
    }
}

// Public interface to threads.
impl Thread {
    /// Start execution of the given thread.
    pub fn start(&self) {
        unsafe { k_thread_start(self.raw) }
    }
}

/// Declare a global static representing a thread variable.
#[macro_export]
macro_rules! kernel_thread_define {
    ($name:ident) => {
        // Since the static object has an atomic that we assume is initialized, let the compiler put
        // this in the data section it finds appropriate (probably .bss if it is initialized to zero).
        // This only matters when the objects are being checked.
        // TODO: This doesn't seem to work with the config.
        // #[cfg_attr(not(CONFIG_RUST_CHECK_KOBJ_INIT),
        //            link_section = concat!(".noinit.", stringify!($name), ".", file!(), line!()))]
        static $name: $crate::object::StaticKernelObject<$crate::raw::k_thread> =
            $crate::object::StaticKernelObject::new();
        // static $name: $crate::sys::thread::Thread = unsafe { ::core::mem::zeroed() };
    };
}

/// For now, this "token" represents the somewhat internal information about thread.
/// What we really want is to make sure that stacks and threads go together.
pub struct StackToken {
    base: *mut z_thread_stack_element,
    size: usize,
}

// This isn't really safe at all, as these can be initialized.  It is unclear how, if even if it is
// possible to implement safe static threads and other data structures in Zephyr.

/// A Statically defined Zephyr `k_thread` object to be used from Rust.
/// 
/// This should be used in a manner similar to:
/// ```
/// const MY_STACK_SIZE: usize = 4096;
///
/// kobj_define! {
///     static MY_THREAD: StaticThread;
///     static MY_STACK: ThreadStack<MY_STACK_SIZE>;
/// }
///
/// let thread = MY_THREAD.spawn(MY_STACK.token(), move || {
///     // Body of thread.
/// });
/// thread.start();
/// ```
pub type StaticThread = StaticKernelObject<k_thread>;

// The thread itself assumes we've already initialized, so this method is on the wrapper.
impl StaticThread {
    /// Spawn this thread to the given external function.  This is a simplified version that doesn't
    /// take any arguments.  The child runs immediately.
    pub fn simple_spawn(&self, stack: StackToken, child: fn() -> ()) -> Thread {
        self.init_help(|raw| {
            unsafe {
                k_thread_create(
                    raw,
                    stack.base,
                    stack.size,
                    Some(simple_child),
                    child as *mut c_void,
                    null_mut(),
                    null_mut(),
                    5,
                    0,
                    K_FOREVER,
                );
            }
        });
        self.get()
    }

    #[cfg(CONFIG_RUST_ALLOC)]
    /// Spawn a thread, running a closure.  The closure will be boxed to give to the new thread.
    /// The new thread runs immediately.
    pub fn spawn<F: FnOnce() + Send + 'static>(&self, stack: StackToken, child: F) -> Thread {
        let child: closure::Closure = Box::new(child);
        let child = Box::into_raw(Box::new(closure::ThreadData {
            closure: ManuallyDrop::new(child),
        }));
        self.init_help(move |raw| {
            unsafe {
                k_thread_create(
                    raw,
                    stack.base,
                    stack.size,
                    Some(closure::child),
                    child as *mut c_void,
                    null_mut(),
                    null_mut(),
                    5,
                    0,
                    K_FOREVER,
                );
            }
        });
        self.get()
    }
}

unsafe extern "C" fn simple_child(
    arg: *mut c_void,
    _p2: *mut c_void,
    _p3: *mut c_void,
) {
    let child: fn() -> () = core::mem::transmute(arg);
    (child)();
}

#[cfg(CONFIG_RUST_ALLOC)]
/// Handle the closure case.  This invokes a double box to rid us of the fat pointer.  I'm not sure
/// this is actually necessary.
mod closure {
    use core::{ffi::c_void, mem::ManuallyDrop};
    use super::Box;

    pub type Closure = Box<dyn FnOnce()>;

    pub struct ThreadData {
        pub closure: ManuallyDrop<Closure>,
    }

    pub unsafe extern "C" fn child(child: *mut c_void, _p2: *mut c_void, _p3: *mut c_void) {
        let mut thread_data: Box<ThreadData> = unsafe { Box::from_raw(child as *mut ThreadData) };
        let closure = unsafe { ManuallyDrop::take(&mut (*thread_data).closure) };
        closure();
    }
}
