# Installation

fusion-kit supports standard CPython 3.11 and later. Wheels include the native extension built with Rust.

## Install from a wheel

Choose the `cp311-abi3` wheel for your platform.

| Platform | Wheel platform tag | CPython versions tested |
|---|---|---|
| Linux x86_64 with glibc 2.28 or later | `manylinux_2_28_x86_64` | 3.11–3.15 |
| Windows x86_64 | `win_amd64` | 3.11, 3.15 |
| macOS arm64 | `macosx_11_0_arm64` | 3.11, 3.15 |
| macOS x86_64 | `macosx_11_0_x86_64` | 3.11, 3.15 |

The macOS wheels use a deployment target of 11.0. Installation tests run on macOS 15.

For example, on Linux x86_64, install the downloaded wheel and check the CLI version.

```bash
python -m pip install ./fusion_kit-0.1.0-cp311-abi3-manylinux_2_28_x86_64.whl
fusion-kit --version
```

## Install from source

You need Rust and a C compiler. The build uses the included `rust-toolchain.toml` and `Cargo.lock`.

```bash
python -m pip install ./fusion_kit-0.1.0.tar.gz
```

To install from a source checkout, run `python -m pip install .` from the repository root.

After installation, import `fusion_kit` in Python or use the `fusion-kit` command. [Python API and CLI reference](api.md)
