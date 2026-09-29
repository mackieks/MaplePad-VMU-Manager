# MaplePad VMU Manager

A Windows desktop app for creating and editing Dreamcast VMU images, built with Rust and egui. Inspired by @pomegd




## Build

Requires Windows x64, [Rust](https://rustup.rs/), and Visual Studio Build Tools with **Desktop development with C++** and the Windows SDK. The pinned Rust toolchain is selected automatically.

```powershell
cargo build --release --locked
```

Run `target/release/maplepad-vmu-manager.exe`. All runtime assets and picotool are embedded; no separate downloads or asset conversion are needed. MaplePad USB access requires a working Raspberry Pi Pico / Pico 2 BOOTSEL driver.

## Development

```powershell
cargo fmt --check
cargo test --release --locked
```

## License

Application code: [MIT](LICENSE). Bundled third-party components are covered separately; see [notices](THIRD_PARTY_NOTICES.md).
