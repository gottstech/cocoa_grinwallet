[![Build Status](https://img.shields.io/travis/gottstech/cocoa_grinwallet/master.svg)](https://travis-ci.org/gottstech/cocoa_grinwallet)

# cocoa_grinwallet
IOS Grin Wallet Pod

## Build
### Set up the environment

- Install Xcode build tools:

`xcode-select --install`

- Install Rust:

`curl https://sh.rustup.rs -sSf | sh`

- Add ios architectures to rustup:

`rustup target add aarch64-apple-ios x86_64-apple-ios armv7s-apple-ios`

- Install `cargo-lipo`, a cargo sub-command for creating iOS libs:

`cargo install cargo-lipo`

### Build the libs

```
git clone --recursive https://github.com/gottstech/cocoa_grinwallet.git
cd cocoa_grinwallet/rust
export OPENSSL_DIR="/usr/local/opt/openssl"
cargo lipo --release --targets aarch64-apple-ios,x86_64-apple-ios,armv7s-apple-ios
./copy_libs.sh
```

Note:
- The generated libs are in `Library/` folder.
- If don't have openssl installed, please run:
  - For Mac: `brew install openssl`
  - For Linux: `sudo apt install libssl-dev`

## License

Apache License v2.0.

## Credits

The code was using the [Ironbelly](https://github.com/cyclefortytwo/ironbelly) as the reference.

The related code taken with thanks and respect, with license details in all derived source files.

Both Ironbelly and this project, are using same open source licence: Apache Licence v2.0.


