# Reopen

[![Travis Build Status](https://api.travis-ci.org/vorner/reopen.png?branch=master)](https://travis-ci.org/vorner/reopen)
[![AppVeyor Build status](https://ci.appveyor.com/api/projects/status/956fq0suxa0x5afi/branch/master?svg=true)](https://ci.appveyor.com/project/vorner/reopen/branch/master)

A tiny `Read`/`Write` wrapper that can reopen the underlying IO object.

The main motivation is integration of logging with logrotate. Usually, when
logrotate wants to rotate log files, it moves the current log file to a new
place and creates a new empty file. However, for the new messages to appear in
the new file, a running program needs to close and reopen the file. This is
most often signalled by SIGHUP.

This allows reopening the IO object used inside the logging drain at runtime.

An example is in the [documentation](https://docs.rs/reopen).

## License

Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms
or conditions.
