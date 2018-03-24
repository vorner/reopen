#![doc(html_root_url = "https://docs.rs/reopen/0.1.0/reopen/")]
#![warn(missing_docs)]

//!  A tiny `Read`/`Write` wrapper that can reopen the underlying IO object.
//!
//! The main motivation is integration of logging with logrotate. Usually, when
//! logrotate wants to rotate log files, it moves the current log file to a new
//! place and creates a new empty file. However, for the new messages to appear in
//! the new file, a running program needs to close and reopen the file. This is
//! most often signalled by SIGHUP.
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

extern crate libc;

use std::io::{Error, Read, Write};
use std::mem;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug)]
pub struct Handle(Arc<AtomicBool>);

static mut GLOBAL_HANDLE: Option<Handle> = None;

extern fn handler(_: libc::c_int, _: *mut libc::siginfo_t, _: *mut libc::c_void) {
    let handle = unsafe { GLOBAL_HANDLE.as_ref().unwrap().clone() };
    handle.reopen();
}

impl Handle {
    pub fn reopen(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn stub() -> Self {
        Handle(Arc::new(AtomicBool::new(true)))
    }
    pub unsafe fn register_signal(&self, signal: libc::c_int) -> Result<(), Error> {
        let mut new: libc::sigaction = mem::zeroed();
        new.sa_sigaction = handler as usize;
        #[cfg(target_os = "android")]
        fn flags() -> libc::c_ulong {
            (libc::SA_RESTART as libc::c_ulong) |
                libc::SA_SIGINFO |
                (libc::SA_NOCLDSTOP as libc::c_ulong)
        }
        #[cfg(not(target_os = "android"))]
        fn flags() -> libc::c_int {
            libc::SA_RESTART |
                libc::SA_SIGINFO |
                libc::SA_NOCLDSTOP
        }
        new.sa_flags = flags();
        if libc::sigaction(signal, &new, ptr::null_mut()) == 0 {
            GLOBAL_HANDLE = Some(self.clone());
            Ok(())
        } else {
            Err(Error::last_os_error())
        }
    }
}

pub struct Reopen<FD> {
    signal: Arc<AtomicBool>,
    constructor: Box<Fn() -> Result<FD, Error> + Send>,
    fd: Option<FD>,
}

impl<FD> Reopen<FD> {
    pub fn new(constructor: Box<Fn() -> Result<FD, Error> + Send>) -> Self {
        Self {
            signal: Arc::new(AtomicBool::new(true)),
            constructor,
            fd: None,
        }
    }
    pub fn handle(&self) -> Handle {
        Handle(Arc::clone(&self.signal))
    }
    fn check(&mut self) -> Result<&mut FD, Error> {
        if self.signal.load(Ordering::Relaxed) {
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
