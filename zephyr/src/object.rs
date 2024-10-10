//! # Zephyr Kernel Objects
//!
//! Zephyr has a concept of a 'kernel object' that is handled a bit magically.  In kernel mode
//! threads, these are just pointers to the data structures that Zephyr uses to manage that item.
//! In userspace, they are still pointers, but those data structures aren't accessible to the
//! thread.  When making syscalls, the kernel validates that the objects are both valid kernel
//! objects and that the are supposed to be accessible to this thread.
//!
//! In many Zephyr apps, the kernel objects in the app are defined as static, using special macros.
//! These macros make sure that the objects get registered so that they are accessible to userspace
//! (at least after that access is granted).
//!
//! There are also kernel objects that are synthesized as part of the build.  Most notably, there
//! are ones generated by the device tree.
//!
//! There are some funny rules about references and mutable references to memory that is
//! inaccessible.  Basically, it is never valid to have a reference to something that isn't
//! accessible.  However, we can have `*mut ktype` or `*const ktype`.  In Rust, having multiple
//! `*mut` pointers live is undefined behavior.  Zephyr makes extensive use of shared mutable
//! pointers (or sometimes immutable).  We will not dereference these in Rust code, but merely pass
//! them to APIs in Zephyr that require them.
//!
//! Most kernel objects use mutable pointers, and it makes sense to require the wrapper structure
//! to be mutable to these accesses.  There a few cases, mostly generated ones that live in
//! read-only memory, notably device instances, that need const pointers.  These will be
//! represented by a separate wrapper.
//!
//! # Initialization tracking
//!
//! The Kconfig `CONFIG_RUST_CHECK_KOBJ_INIT` enabled extra checking in Rust-based kernel objects.
//! This will result in a panic if the objects are used before the underlying object has been
//! initialized.  The initialization must happen through the `StaticKernelObject::init_help`
//! method.
//!
//! TODO: Document how the wrappers work once we figure out how to implement them.

use core::{cell::UnsafeCell, mem};

#[cfg(CONFIG_RUST_CHECK_KOBJ_INIT)]
use crate::sync::atomic::{AtomicUsize, Ordering};

/// A kernel object represented statically in Rust code.
///
/// These should not be declared directly by the user, as they generally need linker decorations to
/// be properly registered in Zephyr as kernel objects.  The object has the underlying Zephyr type
/// T, and the wrapper type W.
///
/// TODO: Handling const-defined alignment for these.
pub struct StaticKernelObject<T> {
    /// The underlying zephyr kernel object.
    value: UnsafeCell<T>,
    /// Initialization status of this object.  Most objects will start uninitialized and be
    /// initialized manually.
    #[cfg(CONFIG_RUST_CHECK_KOBJ_INIT)]
    init: AtomicUsize,
}

/// A kernel object that has a way to get a raw pointer.
///
/// Generally, kernel objects in Rust are just containers for raw pointers to an underlying kernel
/// object.  When this is the case, this trait can be used to get the underlying pointer.
pub trait KobjGet<T> {
    /// Fetch the raw pointer from this object.
    fn get_ptr(&self) -> *mut T;
}

impl<T> KobjGet<T> for StaticKernelObject<T> {
    fn get_ptr(&self) -> *mut T {
        self.value.get()
    }
}

/// A state indicating an uninitialized kernel object.
///
/// This must be zero, as kernel objects will
/// be represetned as zero-initialized memory.
pub const KOBJ_UNINITIALIZED: usize = 0;

/// A state indicating a kernel object that is being initialized.
pub const KOBJ_INITING: usize = 1;

/// A state indicating a kernel object that has completed initialization.
pub const KOBJ_INITIALIZED: usize = 2;

impl<T> StaticKernelObject<T> {
    /// Construct an empty of these objects, with the zephyr data zero-filled.  This is safe in the
    /// sense that Zephyr we track the initialization, and they start in the uninitialized state.
    pub const fn new() -> StaticKernelObject<T> {
        StaticKernelObject {
            value: unsafe { mem::zeroed() },
            #[cfg(CONFIG_RUST_CHECK_KOBJ_INIT)]
            init: AtomicUsize::new(KOBJ_UNINITIALIZED),
        }
    }

    /// An initialization helper.  Runs the code in `f` if the object is uninitialized. Panics if
    /// the initialization state is incorrect.
    #[cfg(CONFIG_RUST_CHECK_KOBJ_INIT)]
    pub fn init_help<R, F: FnOnce(*mut T) -> R>(&self, f: F) -> R {
        if let Err(_) = self.init.compare_exchange(
            KOBJ_UNINITIALIZED,
            KOBJ_INITING,
            Ordering::AcqRel,
            Ordering::Acquire)
        {
            panic!("Duplicate kobject initialization");
        }
        let result = f(self.get_ptr());
        self.init.store(KOBJ_INITIALIZED, Ordering::Release);
        result
    }

    /// An initialization helper.  Runs the code in `f` if the object is uninitialized. Panics if
    /// the initialization state is incorrect.
    #[cfg(not(CONFIG_RUST_CHECK_KOBJ_INIT))]
    pub fn init_help<R, F: FnOnce(*mut T) -> R>(&self, f: F) -> R {
        f(self.get_ptr())
    }
}

/// Kernel object wrappers implement this trait so construct themselves out of the underlying
/// pointer.
pub trait KobjInit<T, W> where Self: KobjGet<T> + Sized {
    /// Get an instance of the kernel object.
    ///
    /// This instance is wrapped in the kernel static object wreapper.  Generally, the trait
    /// implementation is sufficient.
    fn get(&self) -> W {
        let ptr = self.get_ptr();
        Self::wrap(ptr)
    }

    /// Wrap the underlying pointer itself.
    fn wrap(ptr: *mut T) -> W;
}

/// Declare a static kernel object.  This helps declaring static values of Zephyr objects.
///
/// This can typically be used as:
/// ```
/// kobj_define! {
///     static A_MUTEX: StaticMutex;
///     static MUTEX_ARRAY: [StaticMutex; 4];
/// }
/// ```
#[macro_export]
macro_rules! kobj_define {
    ($v:vis static $name:ident: $type:tt; $($rest:tt)*) => {
        $crate::_kobj_rule!($v, $name, $type);
        $crate::kobj_define!($($rest)*);
    };
    ($v:vis static $name:ident: $type:tt<$size:ident>; $($rest:tt)*) => {
        $crate::_kobj_rule!($v, $name, $type<$size>);
        $crate::kobj_define!($($rest)*);
    };
    ($v:vis static $name:ident: $type:tt<$size:literal>; $($rest:tt)*) => {
        $crate::_kobj_rule!($v, $name, $type<$size>);
        $crate::kobj_define!($($rest)*);
    };
    ($v:vis static $name:ident: $type:tt<{$size:expr}>; $($rest:tt)*) => {
        $crate::_kobj_rule!($v, $name, $type<{$size}>);
        $crate::kobj_define!($($rest)*);
    };
    () => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! _kobj_rule {
    ($v:vis, $name:ident, StaticMutex) => {
        #[link_section = concat!("._k_mutex.static.", stringify!($name), ".", file!(), line!())]
        $v static $name: $crate::sys::sync::StaticMutex =
            $crate::sys::sync::StaticMutex::new();
    };
    ($v:vis, $name:ident, [StaticMutex; $size:expr]) => {
        #[link_section = concat!("._k_mutex.static.", stringify!($name), ".", file!(), line!())]
        $v static $name: [$crate::sys::sync::StaticMutex; $size] =
            // This isn't Copy, intentionally, so initialize the whole thing with zerored memory.
            // Relying on the atomic to be 0 for the uninitialized state.
            // [$crate::sys::sync::StaticMutex::new(); $size];
            unsafe { ::core::mem::zeroed() };
    };

    ($v:vis, $name:ident, StaticCondvar) => {
        #[link_section = concat!("._k_condvar.static.", stringify!($name), ".", file!(), line!())]
        $v static $name: $crate::sys::sync::StaticCondvar =
            $crate::sys::sync::StaticCondvar::new();
    };
    ($v:vis, $name:ident, [StaticCondvar; $size:expr]) => {
        #[link_section = concat!("._k_condvar.static.", stringify!($name), ".", file!(), line!())]
        $v static $name: [$crate::sys::sync::StaticCondvar; $size] =
            // This isn't Copy, intentionally, so initialize the whole thing with zerored memory.
            // Relying on the atomic to be 0 for the uninitialized state.
            // [$crate::sys::sync::StaticMutex::new(); $size];
            unsafe { ::core::mem::zeroed() };
    };

    ($v:vis, $name:ident, StaticThread) => {
        // Since the static object has an atomic that we assume is initialized, let the compiler put
        // this in the data section it finds appropriate (probably .bss if it is initialized to zero).
        // This only matters when the objects are being checked.
        // TODO: This doesn't seem to work with the config.
        // #[cfg_attr(not(CONFIG_RUST_CHECK_KOBJ_INIT),
        //            link_section = concat!(".noinit.", stringify!($name), ".", file!(), line!()))]
        $v static $name: $crate::sys::thread::StaticThread =
            $crate::sys::thread::StaticThread::new();
    };

    ($v:vis , $name:ident, [StaticThread; $size:expr]) => {
        $v static $name: [$crate::sys::thread::StaticThread; $size] =
            // See above for the zereod reason.
            unsafe { ::core::mem::zeroed() };
    };

    // Stack expressions have the same syntax ambiguities that they do. We allow an identifier
    // (const), a numeric literal, or an expression in braces.
    ($v:vis, $name:ident, ThreadStack<$size:literal>) => {
        $crate::_kobj_stack!($v, $name, $size);
    };
    ($v:vis, $name:ident, ThreadStack<$size:ident>) => {
        $crate::_kobj_stack!($v, $name, $size);
    };
    ($v:vis, $name:ident, ThreadStack<{$size:expr}>) => {
        $crate::_kobj_stack!($v, $name, $size);
    };

    // Array of stack object versions.
    ($v:vis, $name:ident, [ThreadStack<$size:literal>; $asize:expr]) => {
        $crate::_kobj_stack!($v, $name, $size, $asize);
    };
    ($v:vis, $name:ident, [ThreadStack<$size:ident>; $asize:expr]) => {
        $crate::_kobj_stack!($v, $name, $size, $asize);
    };
    ($v:vis, $name:ident, [ThreadStack<{$size:expr}>; $asize:expr]) => {
        $crate::_kobj_stack!($v, $name, $size, $asize);
    };

    // Queues.
    ($v:vis, $name: ident, StaticQueue) => {
        #[link_section = concat!("._k_queue.static.", stringify!($name), ".", file!(), line!())]
        $v static $name: $crate::sys::queue::StaticQueue =
            unsafe { ::core::mem::zeroed() };
    };

    ($v:vis, $name: ident, [StaticQueue; $size:expr]) => {
        #[link_section = concat!("._k_queue.static.", stringify!($name), ".", file!(), line!())]
        $v static $name: [$crate::sys::queue::StaticQueue; $size] =
            unsafe { ::core::mem::zeroed() };
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! _kobj_stack {
    ($v:vis, $name:ident, $size:expr) => {
        #[link_section = concat!(".noinit.", stringify!($name), ".", file!(), line!())]
        $v static $name: $crate::sys::thread::ThreadStack<{$crate::sys::thread::stack_len($size)}>
            = unsafe { ::core::mem::zeroed() };
    };

    ($v:vis, $name:ident, $size:expr, $asize:expr) => {
        #[link_section = concat!(".noinit.", stringify!($name), ".", file!(), line!())]
        $v static $name: [$crate::sys::thread::ThreadStack<{$crate::sys::thread::stack_len($size)}>; $asize]
            = unsafe { ::core::mem::zeroed() };
    };
}
