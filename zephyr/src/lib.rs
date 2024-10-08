// Copyright (c) 2024 Linaro LTD
// SPDX-License-Identifier: Apache-2.0

//! # Zephyr application support for Rust
//!
//! Rust is a systems programming language focused on safety, speed, and concurrency. Designed to
//! prevent common programming errors such as null pointer deferencing and buffer overflows, Rust
//! emphasizes memory safety without sacrificing performance. Its powerful type system and
//! ownership model ensure thread-safe programming, making it an ideal choice for developing
//! reliable and efficient low-level code. Rust's expressive syntax and modern features make it a
//! robust alternative for developers working on embedded systems, operating systems, and other
//! performance-critial applications.
//!
//! # Enabling Rust Support
//!
//! Zephyr currently supports applications that are written in Rust and C. To enable Rust support,
//! you must select the `CONFIG_RUST` option in the application configuration file.
//!
//! The Rust toolchain is separate from the rest of the Zephyr SDK.  It is recommended to use the
//! [rustup](https://rustup.rs) tool to install the Rust toolchain. In addition to the base
//! compiler, you will need to install core libraries for the target(s) you wish to compile. The
//! easiest way to determine what needs to be installed is to attempt a build. Compilation
//! diagnostics will indicate the `rustup` command needed to install the appropriate target
//! support:
//!
//! ```text
//! $ west build ...
//! ...
//! error[E0463]: can't find crate for `core`
//!   |
//! = note: the `thumbv7m-none-eabi` target may not be installed
//! = help: consider downloading the target with `rustup target add thumbv7-none-eabi`
//! ```
//!
//! in this case, the provided `rustup` command will install the necessary target support. The
//! target required depends on both the board selected and certain configuration choices, such as
//! whether floating point is enabled.
//!
//! # Writing a Rust Application
//!
//! See the `samples` directory in the zephyr-lang-rust repo for examples of Rust applications. The
//! cmake build system is used to build the majority of Zephyr. The declarations in the sample
//! `CMakeLists.txt` files will show how to have cmake invoke `cargo build` at the right time, with
//! the right settings to build for your target.  The rest of the directory is a typical Rust
//! crate, with a few special caveats:
//!
//! * The crate must currently be named `"rustapp"`. This is so that the cmake rules can find the
//!   build.
//! * The crate must be a staticlib crate.
//! * The crate must depend on a "zephyr" crate.  This crate does not come from crates.io, but will
//!   be located by cmake.  This documentation is for this "zephyr" crate.
//!
//! cmake and/or the `west build` command will place the build in a directory (`build` by default)
//! and the rules will direct cargo to also place its build output there (`build/rust/target` for
//! the majority of the output).
//!
//! The build process will also generate a template for a `.cargo/config.toml` that will configure
//! cargo so that tools such as `cargo check` and `rust analyzer` will work with your project.  You
//! can make a symlink for this file to allow these tools to work
//!
//! ```bash
//! $ mkdir .cargo
//! $ cd .cargo
//! $ ln -s ../build/rust/sample-cargo-config.toml config.coml
//! $ cd ..
//! $ cargo check
//! ```
//!
//! # Zephyr Functionality
//!
//! The bindings to Rust for Zephyr are still under development and are currently minimal. However,
//! some Zephyr functionality is available.
//!
//! ## Bool Kconfig Settings
//!
//! Boolean Kconfig settings can be accessed from within Rust code. However, since Rust requires
//! certain compilation decisions, accessing these with `#[cfg...]` directives requires a small
//! addition to `build.rs`. See the docs in the [`zephyr-build` crate](../zephyr-build/index.html).
//!
//! # Other Kconfig Settings
//!
//! All boolean, numeric, and string Kconfig settings are available through the `kconfig` module.

#![no_std]
#![allow(unexpected_cfgs)]

pub mod sys;
pub mod time;

// Bring in the generated kconfig module
include!(concat!(env!("OUT_DIR"), "/kconfig.rs"));

// Ensure that Rust is enabled.
#[cfg(not(CONFIG_RUST))]
compile_error!("CONFIG_RUST must be set to build Rust in Zephyr");

// Printk is provided if it is configured into the build.
#[cfg(CONFIG_PRINTK)]
pub mod printk;

use core::panic::PanicInfo;

/// Override rust's panic.  This simplistic initial version just hangs in a loop.
#[panic_handler]
fn panic(info :&PanicInfo) -> ! {
    #[cfg(CONFIG_PRINTK)]
    {
        printkln!("panic: {}", info);
    }
    let _ = info;

    // Call into the wrapper for the system panic function.
    unsafe {
        extern "C" {
            fn rust_panic_wrap() -> !;
        }
        rust_panic_wrap();
    }
}

/// Re-export of zephyr-sys as `zephyr::raw`.  Generally, most users of zephyr will use
/// `zephyr::raw` instead of directly importing the zephyr-sys crate.
pub mod raw {
    pub use zephyr_sys::*;
}

/// Provide symbols used by macros in a crate-local namespace.
#[doc(hidden)]
pub mod _export {
    pub use core::format_args;
}
