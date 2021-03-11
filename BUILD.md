
# Building the Geobacter Rust Compiler

In a checkout of Geobacter, run:
`cargo run --manifest-path rust-builder/Cargo.toml -- --target-dir ../geobacter-rust-build`.
This will automatically checkout the Geobacter Rust sources, setup a config with the specific
options we need, build Geobacter's Rust fork, and then setup `rustup` with
a local toolchain named `geobacter`.

That's it!
