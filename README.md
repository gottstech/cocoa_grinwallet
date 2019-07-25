[![Build Status](https://img.shields.io/travis/gottstech/cocoa_grinwallet/master.svg)](https://travis-ci.org/gottstech/cocoa_grinwallet)

# cocoa_grinwallet
IOS Grin Wallet Pod

## Build
### Set up the environment

- Install Xcode build tools:

```Bash
xcode-select --install
```

- Install Rust:

`curl https://sh.rustup.rs -sSf | sh`

- Add ios architectures to rustup:

```Bash
rustup target add aarch64-apple-ios x86_64-apple-ios armv7s-apple-ios
```

- Install `cargo-lipo`, a cargo sub-command for creating iOS libs:

```Bash
cargo install cargo-lipo
```

### Build the libs

```Bash
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
  
### On IOS Application Side

Add the following 2 lines into your `Podfile`:
```Bash
  pod 'cocoa_grinwallet', :git => 'https://github.com/gottstech/cocoa_grinwallet.git', :tag => 'v1.0.2'
  pod 'OpenSSL', '~> 1.0'
```
then run `pod install`

If you have problem for OpenSSL pod installation, please refer to [this post](https://stackoverflow.com/a/57196786/3831478) to solve it.  

After the pod installation, remember to manually download the libraries to avoid a long building procedure. The libraries can be found in the [release](https://github.com/gottstech/cocoa_grinwallet/releases) page of this repo.

```Bash
to be completed...
```

## License

Apache License v2.0.

## Credits

The code was using the [Ironbelly](https://github.com/cyclefortytwo/ironbelly) as the reference.

The related code taken with thanks and respect, with license details in all derived source files.

Both Ironbelly and this project, are using same open source licence: Apache Licence v2.0.


