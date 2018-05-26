use std::io::Error;
use std::mem;
use std::ptr;

use libc;

use super::Handle;

static mut GLOBAL_HANDLE: Option<Handle> = None;

extern "C" fn handler(_: libc::c_int, _: *mut libc::siginfo_t, _: *mut libc::c_void) {
    let handle = unsafe { GLOBAL_HANDLE.as_ref().unwrap().clone() };
    handle.reopen();
}

impl Handle {
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
            (libc::SA_RESTART as libc::c_ulong)
                | libc::SA_SIGINFO
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
