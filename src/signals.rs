//! Unix signal handling.
//!
//! Signal handling is hard, and doing it from a library in a way it doesn't break other stuff and
//! in the face of multiple threads harder so. We need to:
//!
//! * Be able to have multiple signals registered.
//! * Attach multiple handles to a single signal.
//! * Somehow survive bad things, like someone calling the signal why we're at the process of
//!   setting it up.
//! * Don't break others' signals, so we want to chain them (eg. when setting up, store the
//!   original and call that).
//! * Avoid using Mutexes and other such things inside a signal handler.
//!
//! The high-level approach here is inspired by RCU. There's a data structure describing the things
//! we should do inside a signal handler (a hash map indexed by the signal number). When our signal
//! is called, we get a snapshot of this structure, go through the handles in there and wake them
//! up. Then we chain-call the original handler. All this is done just using atomic operations,
//! without any locking.
//!
//! The replacement is slightly more complicated. The idea is, we take a snapshot of the structure,
//! extend it, do what needs to be done to register signals and install a new version. However, if
//! two threads did that at once, it would cause confusion and we can't do that. Therefore, this
//! whole operation is mutex-protected. This protects against two parallel modifications, but the
//! reads are not protected and can happen even while we do the update operation.
//!
//! # Unsafe
//!
//! The are two kinds of unsafes. One is to manipulate some mutable global state. As Rust doesn't
//! support `const fn` yet, we need to initialize our global data structures (the ones we'll access
//! from the signal handler) at the first use. This is done by the `Once` primitive and is
//! guaranteed to happen *before* we install the first signal handler. Therefore it is safe to
//! assume it never changes and is initialized in the signal handler.
//!
//! The other kind is FFI, calling of signal, etc.

extern crate arc_swap;
extern crate libc;

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io::Error;
use std::mem;
use std::ptr;
use std::sync::{Arc, Mutex, Once, ONCE_INIT};

use self::arc_swap::ArcSwap;

use super::Handle;

/// Slot for info about one signal.
#[derive(Clone)]
struct SignalSlot {
    /// The previous signal handler. This allows chaining.
    prev: libc::sigaction,

    /// Handles to be notified.
    ///
    /// TODO: Any ability to actually remove them?
    handles: Vec<Handle>,
}

impl SignalSlot {
    /// Creates an empty slot and registers a signal handler for it.
    fn new(signal: libc::c_int) -> Result<Self, Error> {
        // C data structure, expected to be zeroed out.
        let mut new: libc::sigaction = unsafe { mem::zeroed() };
        new.sa_sigaction = handler as usize;
        #[cfg(target_os = "android")]
        fn flags() -> libc::c_ulong {
            (libc::SA_RESTART as libc::c_ulong)
                | libc::SA_SIGINFO
                | (libc::SA_NOCLDSTOP as libc::c_ulong)
        }
        #[cfg(not(target_os = "android"))]
        fn flags() -> libc::c_int {
            libc::SA_RESTART | libc::SA_SIGINFO | libc::SA_NOCLDSTOP
        }
        new.sa_flags = flags();
        // C data structure, expected to be zeroed out.
        let mut old: libc::sigaction = unsafe { mem::zeroed() };
        // FFI ‒ pointers are valid, it doesn't take ownership.
        if unsafe { libc::sigaction(signal, &new, &mut old) } != 0 {
            return Err(Error::last_os_error());
        }
        Ok(SignalSlot {
            prev: old,
            handles: Vec::new(),
        })
    }
}

/// Slots for all the signal slots.
type AllSignals = HashMap<libc::c_int, SignalSlot>;

struct GlobalData {
    all_signals: ArcSwap<AllSignals>,
    rcu_lock: Mutex<()>,
}

static mut GLOBAL_DATA: Option<GlobalData> = None;
static GLOBAL_INIT: Once = ONCE_INIT;

impl GlobalData {
    /// Gets the value of current global data.
    fn get() -> &'static Self {
        // Accessing of global data. Called only after ensure run at least once.
        unsafe { GLOBAL_DATA.as_ref().unwrap() }
    }
    /// Makes sure the global data is initialized and gets it.
    fn ensure() -> &'static Self {
        // Setting the global data, exactly once.
        GLOBAL_INIT.call_once(|| unsafe {
            GLOBAL_DATA = Some(GlobalData {
                all_signals: ArcSwap::from(Arc::new(HashMap::new())),
                rcu_lock: Mutex::new(()),
            });
        });
        Self::get()
    }
}

extern "C" fn handler(sig: libc::c_int, info: *mut libc::siginfo_t, data: *mut libc::c_void) {
    let signals = GlobalData::get().all_signals.load();
    if let Some(ref sigdata) = signals.get(&sig) {
        for handle in &sigdata.handles {
            handle.reopen();
        }

        let fptr = sigdata.prev.sa_sigaction;
        if fptr != 0 && fptr != libc::SIG_DFL && fptr != libc::SIG_IGN {
            // FFI ‒ calling the original signal handler.
            unsafe {
                if sigdata.prev.sa_flags & libc::SA_SIGINFO == 0 {
                    let action = mem::transmute::<usize, extern "C" fn(libc::c_int)>(fptr);
                    action(sig);
                } else {
                    type SigAction =
                        extern "C" fn(libc::c_int, *mut libc::siginfo_t, *mut libc::c_void);
                    let action = mem::transmute::<usize, SigAction>(fptr);
                    action(sig, info, data);
                }
            }
        }
    }
}

fn block_signal(signal: libc::c_int) -> Result<libc::sigset_t, Error> {
    unsafe {
        let mut newsigs: libc::sigset_t = mem::uninitialized();
        libc::sigemptyset(&mut newsigs);
        libc::sigaddset(&mut newsigs, signal);
        let mut oldsigs: libc::sigset_t = mem::uninitialized();
        libc::sigemptyset(&mut oldsigs);
        if libc::sigprocmask(libc::SIG_BLOCK, &newsigs, &mut oldsigs) == 0 {
            Ok(oldsigs)
        } else {
            Err(Error::last_os_error())
        }
    }
}

fn restore_signals(signals: libc::sigset_t) -> Result<(), Error> {
    if unsafe { libc::sigprocmask(libc::SIG_SETMASK, &signals, ptr::null_mut()) } == 0 {
        Ok(())
    } else {
        Err(Error::last_os_error())
    }
}

impl Handle {
    /// Installs a signal handler to invoke the reopening when a certain signal comes.
    ///
    /// # Notes
    ///
    /// * This installs a signal handler. Signal handlers are program-global entities, so you may
    ///   be careful.
    /// * If there are multiple handles for the same signal, they share their signal handler ‒ only
    ///   the first one for each signal registers one.
    /// * Upon signal registration, the original handler is stored and called in chain from our own
    ///   signal handler.
    /// * A single handle can be used for multiple signals.
    /// * It is not (currently) possible to unregister a handle once it's been registered. While an
    ///   orphaned handle (one whose Reopen was dropped) doesn't do any harm, it still takes some
    ///   space in memory. Therefore registering and forgetting handles in a loop might not be a
    ///   good idea.
    ///
    /// # Race condition
    ///
    /// Currently, there's a short race condition. If there was a previous signal handler and a
    /// signal comes into a different thread during the process of installing our own, it may
    /// happen neither ours nor the original is called. The practical effect of this is considered
    /// rather unimportant, as most programs install their signal handlers early at startup, before
    /// they have chance to do anything useful. Still, if there are ideas how to avoid that, they
    /// are welcome.
    ///
    /// Note that installing the signal handler before starting any threads eliminates the race
    /// condition.
    ///
    /// # Error handling
    ///
    /// Note that if this function returns an error, there's no guarantee about what signal
    /// handlers were or were not set up or if the handle is registered for signal handling.
    pub fn register_signal(&self, signal: libc::c_int) -> Result<(), Error> {
        let globals = GlobalData::ensure();
        let _lock = globals.rcu_lock.lock().unwrap();
        // Make sure to manipulate the signal mask *inside* the lock. Doing that outside might have
        // odd consequences. We do this to avoid the race condition at our own thread ‒ if someone
        // installs the signal handlers before starting any threads, it should eliminate the
        // condition.
        let old_signals = block_signal(signal)?;
        let mut signals = AllSignals::clone(&globals.all_signals.load());
        match signals.entry(signal) {
            Entry::Occupied(mut occupied) => occupied.get_mut().handles.push(self.clone()),
            Entry::Vacant(vacant) => match SignalSlot::new(signal) {
                Ok(mut slot) => {
                    slot.handles.push(self.clone());
                    vacant.insert(slot);
                }
                Err(e) => {
                    // In case there are errors both from signal restoration and from signal
                    // registration, we prefer the latter.
                    drop(restore_signals(old_signals));
                    return Err(e);
                }
            },
        }
        globals.all_signals.store(Arc::new(signals));
        restore_signals(old_signals)
    }
}
