//! Example of reopening log file on SIGHUP
//!
//! This program keeps writing messages into a file `log.txt`. If it receives SIGHUP, it reopens
//! it.
//!
//! To demonstrate the effect:
//!
//! * Run the program.
//! * Observe `log.txt` appeared and it is growing.
//! * Move the `log.txt` to some other file (`mv log.txt log2.txt`).
//! * See that the file is still growing, even when under different name.
//! * Send `SIGHUP` to the program (`killall -SIGHUP reopen_log`).
//! * See `log2.txt` no longer grows, new `log.txt` appeared and grows.

use std::fs::File;
use std::io::{Error, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

use reopen::Reopen;

/// Keeps writing into the given file (or, `Write`), one line per second.
fn log_forever<W: Write>(mut w: W) -> Result<(), Error> {
    let mut no = 1u128;
    loop {
        thread::sleep(Duration::from_secs(1));
        writeln!(w, "Tick no {}", no)?;
        no += 1;
    }
}

/// Open file at given path for writing, creating if necessary.
fn open_log<P: AsRef<Path>>(p: P) -> Result<File, Error> {
    File::create(p)
}

fn main() -> Result<(), Error> {
    // Create a proxy to the file
    let log = Reopen::new(Box::new(|| open_log("log.txt")))?;
    // Make sure it gets reopened on SIGHUP
    log.handle().register_signal(libc::SIGHUP)?;
    // Pass it to the logging facility
    log_forever(log)
}
