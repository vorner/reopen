* Migrated to edition 2018, fixed the low Rust version to 1.31.0

# 0.3.0

* Delegated the signal handling to the signal-hook crate, so the same signal can
  be shared with other things and the code in this crate is simplified (breaking
  change in the `register_signal` method return type).
* Fixed a bug with extra reopen just after creation.

# 0.2.1

* Lifted the many annoying limitations of `Handle::register_signal`.
* Made the `Handle::register_signal` function safe.
* Added an example.

# 0.2.0

* Minor fixes in documentation links.
* Error handling improvements:
  - Better documentation for what happens.
  - Perform first opening in the constructor, getting a potential serious error
    on the first try.

# Older versions

* ?? No historical records…
