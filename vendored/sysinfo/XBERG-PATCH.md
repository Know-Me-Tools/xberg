# sysinfo 0.39.6 iOS compatibility patch

Upstream source: [sysinfo v0.39.6](https://github.com/GuillaumeGomez/sysinfo/tree/1bc6ed4f7d0dd03606de572a1531a165438b63ec), commit `1bc6ed4f7d0dd03606de572a1531a165438b63ec`.
The package source is copied byte-for-byte from backport commit `70731a55a615fffd7f46a36613fed4220931c8c3`.
The original standalone `Cargo.toml` is retained rather than Cargo's normalized registry manifest.

`xberg-ios-network.patch` is the exact diff from that upstream commit to the backport; its SHA-256 is `f8c1c8bc3ad6ad76f95f65c7e84163779472f292193f0ada077adc766bc04ecb`.
It changes only `src/lib.rs`, `src/unix/mod.rs`, `src/unix/apple/mod.rs`, and `tests/network.rs`.
The backport uses the existing unsupported-network implementation on iOS, where the network APIs used by 0.39 are unavailable, and preserves the 0.39 API and macOS implementation.

The copy includes 142 upstream package source files, including the MIT license, documentation includes, examples, and tests.
Generated package metadata and lock files, Git ignore metadata, and upstream CI configuration are omitted.

Remove this directory and the Cargo override once the Hugging Face/Xet dependency chain accepts a published sysinfo version with the iOS fix.
