// Copyright (c) 2023 Linaro LTD
// SPDX-License-Identifier: Apache-2.0

#![no_std]

// Cargo tries to detect configs that have typos in them.  Unfortunately, the Zephyr Kconfig system
// uses a large number of Kconfigs and there is no easy way to know which ones might conceivably be
// valid.  This prevents a warning about each cfg that is used.
#![allow(unexpected_cfgs)]

extern crate alloc;

#[allow(unused_imports)]
use alloc::boxed::Box;
use alloc::vec::Vec;
use zephyr::object::KobjInit;
use zephyr::time::{Duration, sleep, Tick};
use zephyr::{
    printkln,
    kobj_define,
    sys::uptime_get,
    sync::{Arc, Mutex},
};

// These are optional, based on Kconfig, so allow them to be unused.
#[allow(unused_imports)]
use crate::condsync::CondSync;
#[allow(unused_imports)]
use crate::sysmutex::SysMutexSync;
#[allow(unused_imports)]
use crate::channel::get_channel_syncer;

mod condsync;
mod sysmutex;
mod channel;

/// How many philosophers.  There will be the same number of forks.
const NUM_PHIL: usize = 6;

/// How much stack should each philosopher thread get.  Worst case I've seen is riscv64, with 3336
/// bytes, when printing messages.  Make a bit larger to work.
const PHIL_STACK_SIZE: usize = 4096;

// The dining philosophers problem is a simple example of cooperation between multiple threads.
// This implementation use one of several different underlying mechanism to support this cooperation.

// This example uses dynamic dispatch to allow multiple implementations.  The intent is to be able
// to periodically shut down all of the philosphers and start them up with a differernt sync
// mechanism.  This isn't implemented yet.

/// The philosophers use a fork synchronization mechanism.  Essentially, this is 6 locks, and will be
/// implemented in a few different ways to demonstrate/test different mechanmism in Rust.  All of
/// them implement The ForkSync trait which provides this mechanism.
trait ForkSync: core::fmt::Debug + Sync + Send {
    /// Take the given fork.  The are indexed the same as the philosopher index number.  This will
    /// block until the fork is released.
    fn take(&self, index: usize);

    /// Release the given fork.  Index is the same as take.
    fn release(&self, index: usize);
}

#[no_mangle]
extern "C" fn rust_main() {
    printkln!("Hello world from Rust on {}",
              zephyr::kconfig::CONFIG_BOARD);
    printkln!("Time tick: {}", zephyr::time::SYS_FREQUENCY);

    STAT_MUTEX.init();
    let stats = Arc::new(Mutex::new_from(Stats::default(), STAT_MUTEX.get()));

    let syncers = get_syncer();

    printkln!("Pre fork");
    for (i, syncer) in (0..PHIL_THREAD.len()).zip(syncers.into_iter()) {
        /*
        let child_syncer = syncer.clone();
        */
        // The Rust borrow checker doesn't seem quite smart enough to realize that we are moving
        // these out individually.  Best would be to rewrite this to iterate over the queue, but for
        // now, the clone isn't terribly costly.
        /*
        let child_syncer = ChannelSync::new(cq_send.clone(), reply_queues[i].clone());
        let child_syncer = Arc::new(child_syncer);
        */
        let child_stat = stats.clone();
        let thread = PHIL_THREAD[i].spawn(PHIL_STACK[i].token(), move || {
            phil_thread(i, syncer, child_stat);
        });
        thread.start();
    }

    let delay = Duration::secs_at_least(10);
    loop {
        // Periodically, printout the stats.
        zephyr::time::sleep(delay);
        stats.lock().unwrap().show();
    }
}

#[cfg(CONFIG_SYNC_SYS_MUTEX)]
fn get_syncer() -> Vec<Arc<dyn ForkSync>> {
    // Simple mutex version.
    let syncer = Box::new(SysMutexSync::new())
        as Box<dyn ForkSync>;
    let syncer: Arc<dyn ForkSync> = Arc::from(syncer);
    let mut result = Vec::new();
    for _ in 0..NUM_PHIL {
        result.push(syncer.clone());
    }
    result
}

#[cfg(CONFIG_SYNC_CONDVAR)]
fn get_syncer() -> Vec<Arc<dyn ForkSync>> {
    // Condvar version
    let syncer = Box::new(CondSync::new())
        as Box<dyn ForkSync>;
    let syncer: Arc<dyn ForkSync> = Arc::from(syncer);
    let mut result = Vec::new();
    for _ in 0..NUM_PHIL {
        result.push(syncer.clone());
    }
    result
}

#[cfg(CONFIG_SYNC_CHANNEL)]
fn get_syncer() -> Vec<Arc<dyn ForkSync>> {
    get_channel_syncer()
}

fn phil_thread(n: usize, syncer: Arc<dyn ForkSync>, stats: Arc<Mutex<Stats>>) {
    printkln!("Child {} started: {:?}", n, syncer);

    // Determine our two forks.
    let forks = if n == NUM_PHIL - 1 {
        // Per Dijkstra, the last phyilosopher needs to reverse forks, or we deadlock.
        (0, n)
    } else {
        (n, n+1)
    };

    loop {
        {
            // printkln!("Child {} hungry", n);
            // printkln!("Child {} take left fork", n);
            syncer.take(forks.0);
            // printkln!("Child {} take right fork", n);
            syncer.take(forks.1);

            let delay = get_random_delay(n, 25);
            // printkln!("Child {} eating ({} ms)", n, delay);
            sleep(delay);
            stats.lock().unwrap().record_eat(n, delay);

            // Release the forks.
            // printkln!("Child {} giving up forks", n);
            syncer.release(forks.1);
            syncer.release(forks.0);

            let delay = get_random_delay(n, 25);
            // printkln!("Child {} thinking ({} ms)", n, delay);
            sleep(delay);
            stats.lock().unwrap().record_think(n, delay);
        }
    }
}

/// Instead of just printing out so much information that the data just scrolls by, gather statistics.
#[derive(Default)]
struct Stats {
    /// How many times each philospher has gone through the loop.
    count: [u64; NUM_PHIL],
    /// How much time each philosopher has spent eating.
    eating: [u64; NUM_PHIL],
    /// How much time each pilosopher has spent thinking.
    thinking: [u64; NUM_PHIL],
}

impl Stats {
    fn record_eat(&mut self, index: usize, time: Duration) {
        self.eating[index] += time.to_millis();
    }

    fn record_think(&mut self, index: usize, time: Duration) {
        self.thinking[index] += time.to_millis();
        self.count[index] += 1;
    }

    fn show(&self) {
        printkln!("{:?}, e:{:?}, t:{:?}", self.count, self.eating, self.thinking);

        /*
        // Invoke the thread analyzer report.
        {
            extern "C" {
                fn thread_analyzer_print(cpu: usize);
            }
            unsafe {
                thread_analyzer_print(0);
            }
        }
        */
    }
}

/// Get a random delay, based on the ID of this user, and the current uptime.
fn get_random_delay(id: usize, period: usize) -> Duration {
    let tick = (uptime_get() & (usize::MAX as i64)) as usize;
    let delay = (tick / 100 * (id + 1)) & 0x1f;

    // Use one greater to be sure to never get a delay of zerp.
    Duration::millis_at_least(((delay + 1) * period) as Tick)
}

kobj_define! {
    static PHIL_THREAD: [StaticThread; NUM_PHIL];
    static PHIL_STACK: [ThreadStack<PHIL_STACK_SIZE>; NUM_PHIL];

    // A mutex to hold statistics data.
    static STAT_MUTEX: StaticMutex;
}
