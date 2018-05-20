#![doc(html_root_url = "https://docs.rs/reopen/0.1.2/reopen/")]
#![warn(missing_docs)]

//!  A tiny `Read`/`Write` wrapper that can reopen the underlying IO object.
//!
//! The main motivation is integration of logging with logrotate. Usually, when
//! logrotate wants to rotate log files, it moves the current log file to a new
//! place and creates a new empty file. However, for the new messages to appear in
//! the new file, a running program needs to close and reopen the file. This is
//! most often signalled by SIGHUP.
//!
//! # Examples
//!
//! This allows reopening the IO object used inside the logging drain at runtime.
//!
//! ```rust
//! extern crate libc;
//! #[macro_use]
//! extern crate log;
//! extern crate reopen;
//! extern crate simple_logging;
//!
//! use std::fs::{File, OpenOptions};
//! use std::io::Error;
//!
//! use reopen::Reopen;
//!
//! fn open() -> Result<File, Error> {
//!     OpenOptions::new()
//!         .create(true)
//!         .write(true)
//!         .append(true)
//!         .open("/dev/null")
//! }
//!
//! fn main() {
//!     let file = Reopen::new(Box::new(&open));
//!     // Must be called before any threads are started
//!     unsafe { file.handle().register_signal(libc::SIGHUP).unwrap() };
//!     simple_logging::log_to(file, log::LevelFilter::Debug);
//!     info!("Hey, it's logging");
//! }
//! ```
//!
//! Note that this solution is a bit hacky and probably solves only the most common use case.
//!
//! If you find another use case for it, I'd like to hear about it.

extern crate libc;

use std::io::{Error, Read, Write};
#[cfg(unix)]
use std::mem;
#[cfg(unix)]
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A handle to signal a companion [`Reopen`](struct.Reopen.html) object to do a reopen on its next
/// operation.
#[derive(Clone, Debug)]
pub struct Handle(Arc<AtomicBool>);

static mut GLOBAL_HANDLE: Option<Handle> = None;

#[cfg(unix)]
extern "C" fn handler(_: libc::c_int, _: *mut libc::siginfo_t, _: *mut libc::c_void) {
    let handle = unsafe { GLOBAL_HANDLE.as_ref().unwrap().clone() };
    handle.reopen();
}

impl Handle {
    /// Signals the companion [`Reopen`](struct.Reopen.html) object to do a reopen on its next
    /// operation.
    pub fn reopen(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Creates a useless handle, not paired to anything.
    ///
    /// Note that this useless handle can be added to a new [`Reopen`](struct.Reopen.html) with the
    /// [`with_handle`](struct.Reopen.html#method.with_handle) and becomes useful.
    pub fn stub() -> Self {
        Handle(Arc::new(AtomicBool::new(true)))
    }

    #[cfg(unix)]
    /// Installs a signal handler to invoke the reopening when a certain signal comes.
    ///
    /// # Notes
    ///
    /// * This *replaces* any other signal with the given signal number. It's not really possible
    ///   to reopen multiple things with a single signal in this simple way. If you need that, call
    ///   `reopen` manually.
    /// * There's only one global handle, so no matter how many signals you want to use, it still
    ///   won't handle multiple reopen instances. If you need that, you can either handle signals
    ///   yourself or open a pull request (I'm not against doing it properly, I just didn't need it
    ///   yet).
    /// * And yes, this function is an ugly hack.
    /// * This may be called only before you start any additional threads ‒ best way to place it at
    ///   the start of the `main` function. If any threads are running and accessing the reopen
    ///   object (eg. logging), it invokes undefined behaviour.
    pub unsafe fn register_signal(&self, signal: libc::c_int) -> Result<(), Error> {
        let mut new: libc::sigaction = mem::zeroed();
        new.sa_sigaction = handler as usize;
        #[cfg(target_os = "android")]
        fn flags() -> libc::c_ulong {
            (libc::SA_RESTART as libc::c_ulong) | libc::SA_SIGINFO
                | (libc::SA_NOCLDSTOP as libc::c_ulong)
        }
        #[cfg(not(target_os = "android"))]
        fn flags() -> libc::c_int {
            libc::SA_RESTART | libc::SA_SIGINFO | libc::SA_NOCLDSTOP
        }
        new.sa_flags = flags();
        // Insert it first, so it is ready once we install the signal handler
        let mut original = Some(self.clone());
        mem::swap(&mut original, &mut GLOBAL_HANDLE);
        if libc::sigaction(signal, &new, ptr::null_mut()) == 0 {
            Ok(())
        } else {
            // Return it back to the original if the signal handler failed, whatever it was. That
            // is not very useful likely, but probably more expected.
            mem::swap(&mut original, &mut GLOBAL_HANDLE);
            Err(Error::last_os_error())
        }
    }
}

/// A `Read`/`Write` proxy that can reopen the underlying object.
///
/// It is constructed with a function that can open a new instance of the object. If it is signaled
/// to reopen it (though [`handle`](#method.handle)), it drops the old instance and uses the
/// function to create a new one at the next IO operation.
pub struct Reopen<FD> {
    signal: Arc<AtomicBool>,
    constructor: Box<Fn() -> Result<FD, Error> + Send>,
    fd: Option<FD>,
}

impl<FD> Reopen<FD> {
    /// Creates a new instance.
    pub fn new(constructor: Box<Fn() -> Result<FD, Error> + Send>) -> Self {
        Self::with_handle(Handle::stub(), constructor)
    }

    /// Creates a new instance from the given handle.
    ///
    /// This might come useful if you want to create the handle beforehand with
    /// [`Handle::stub`](struct.Handle.html#method.stub) (eg. in
    /// [`lazy_static`](https://docs.rs/lazy_static)).
    /// Note that using the same handle for multiple `Reopen`s will not work as expected (the first
    /// one to be used resets the signal and the others don't reopen).
    ///
    /// # Examples
    ///
    /// ```
    /// # use reopen::*;
    /// // Something that implements `Write`, for example.
    /// struct Writer;
    ///
    /// let handle = Handle::stub();
    /// let reopen = Reopen::with_handle(handle.clone(), Box::new(|| Ok(Writer)));
    ///
    /// handle.reopen();
    /// ```
    pub fn with_handle(handle: Handle, constructor: Box<Fn() -> Result<FD, Error> + Send>) -> Self {
        Self {
            signal: handle.0,
            constructor,
            fd: None,
        }
    }

    /// Returns a handle to signal this `Reopen` to perform the reopening.
    pub fn handle(&self) -> Handle {
        Handle(Arc::clone(&self.signal))
    }

    fn check(&mut self) -> Result<&mut FD, Error> {
        if self.signal.swap(false, Ordering::Relaxed) {
            self.fd.take();
        }
        if self.fd.is_none() {
            self.fd = Some((self.constructor)()?);
        }
        Ok(self.fd.as_mut().unwrap())
    }
}

impl<FD: Read> Read for Reopen<FD> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        self.check().and_then(|fd| fd.read(buf))
    }
}

impl<FD: Write> Write for Reopen<FD> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        self.check().and_then(|fd| fd.write(buf))
    }

    fn flush(&mut self) -> Result<(), Error> {
        self.check().and_then(Write::flush)
    }
}
