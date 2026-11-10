# インストール

fusion-kitは通常版のCPython 3.11以上を対象とします。wheelにはRust製の拡張モジュールが含まれます。

## wheelからインストール

環境に対応する `cp311-abi3` wheelを選びます。

| 環境 | wheelのプラットフォームタグ | インストール試験を行ったCPython |
|---|---|---|
| Linux x86_64、glibc 2.28以降 | `manylinux_2_28_x86_64` | 3.11〜3.15 |
| Windows x86_64 | `win_amd64` | 3.11、3.15 |
| macOS arm64 | `macosx_11_0_arm64` | 3.11、3.15 |
| macOS x86_64 | `macosx_11_0_x86_64` | 3.11、3.15 |

macOSのタグに含まれる11.0はビルド時のdeployment targetです。インストール試験はmacOS 15で行っています。

```bash
python -m pip install ./fusion_kit-0.1.0-cp311-abi3-manylinux_2_28_x86_64.whl
fusion-kit --version
```

## ソースからインストール

RustとCコンパイラを用意します。ビルドには同梱の `rust-toolchain.toml` と `Cargo.lock` を使います。

```bash
python -m pip install ./fusion_kit-0.1.0.tar.gz
```

ソースチェックアウトからは、リポジトリのルートで `python -m pip install .` を実行します。

インストール後はPython APIと `fusion-kit` コマンドが使えます。 [APIとCLIの使い方](api.md)
