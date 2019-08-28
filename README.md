[![Build Status](https://img.shields.io/travis/gottstech/cocoa_grinwallet/master.svg)](https://travis-ci.org/gottstech/cocoa_grinwallet)

# cocoa_grinwallet
IOS Grin Wallet Pod

## How to import this pod

Add one line into your `Podfile`:
```Bash
  pod 'cocoa_grinwallet', :git => 'https://github.com/gottstech/cocoa_grinwallet.git', :tag => 'v1.0.4'
```
then run `pod install`

After the pod installation, remember to manually download the libraries to avoid a long building procedure. The libraries can be found in the [release](https://github.com/gottstech/cocoa_grinwallet/releases) page of this repo.

<details>
 <summary>download script</summary>

```Bash
#!/bin/bash

version=`grep " pod 'cocoa_grinwallet'" Podfile | sed "s/.*:tag => '\(.*\)'/\1/"`

mkdir -p Pods/cocoa_grinwallet/cocoa_grinwallet/Library && cd Pods/cocoa_grinwallet/cocoa_grinwallet/Library && rm -f libgrinwallet* || exit 1

wget https://github.com/gottstech/cocoa_grinwallet/releases/download/${version}/libgrinwallet_aarch64-apple-ios.a || exit 1

wget https://github.com/gottstech/cocoa_grinwallet/releases/download/${version}/libgrinwallet_armv7s-apple-ios.a || exit 1

wget https://github.com/gottstech/cocoa_grinwallet/releases/download/${version}/libgrinwallet_x86_64-apple-ios.a || exit 1

printf "3 libs have been downloaded successfully\n"

cd - > /dev/null || exit 1
ls -l Pods/cocoa_grinwallet/cocoa_grinwallet/Library
```
</details>

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
cargo lipo --release --targets aarch64-apple-ios,x86_64-apple-ios,armv7s-apple-ios
./copy_libs.sh
```

Note:
- The generated libs are in `Library/` folder.  

## Document

https://github.com/gottstech/cocoa_grinwallet/wiki

- [[Grin Wallet Cocoa API Guide|Grin-Wallet-Cocoa-API-Guide]]
- [Wallet Address Specification](https://github.com/gigglewallet/grinrelay/wiki/Bech32-Grin-Relay-Address)
- [Wallet Transaction Security on Grin Relay](https://github.com/gigglewallet/grinrelay/wiki/GrinRelay-Security-Specification)

## License

Apache License v2.0.

## Credits

The code use the [Ironbelly](https://github.com/cyclefortytwo/ironbelly) as the initial reference.

The related code taken with thanks and respect, with license details in all derived source files.

Both Ironbelly and this project, are using same open source licence: Apache Licence v2.0.


