# Installation

Carabiner is a Rust command line application. Install the published crate with Cargo.

## Install from crates.io

Install Rust and Cargo first with [rustup](https://rustup.rs/), then run:

```bash
cargo install carabiner --locked
```

!!! note
    Cargo installs the `carabiner` executable in its bin directory, which is usually `$HOME/.cargo/bin`. Add that directory to your `PATH` if the command is not available in a new terminal.

Verify the installation:

```bash
carabiner --version
carabiner --help
```

## Build from Source

Clone the repository and build the release binary:

```bash
git clone https://github.com/findyourexit/carabiner.git
cd carabiner
cargo build --release --locked
./target/release/carabiner --version
```

To install the binary built from the checkout into Cargo's bin directory, run:

```bash
cargo install --path . --locked
```
