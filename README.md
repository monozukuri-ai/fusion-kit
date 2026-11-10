# fusion-kit

Autodesk FusionのF3D・F3Zをローカルで読み取るPythonパッケージです。Rust製のパーサをPython APIとCLIから利用できます。実行時にAutodesk Fusionやクラウドサービスは不要です。

- ZIPメンバーの一覧・読取と、展開サイズ・CRCの検証。圧縮方式はStored、Deflate、Zstandardに対応。
- F3DのManifest、アセットとセグメント、プレビュー画像の取得。
- F3Zに含まれる文書の一覧、ルート文書、XREF・DERIVED参照の取得と整合性検査。

## インストール

通常版のCPython 3.11以上が必要です。対象環境のwheelを選ぶか、RustとCコンパイラを使ってソースからインストールできます。 [対応環境とインストール手順](docs/distribution.md)

## 使い方

```python
import fusion_kit

archive = fusion_kit.readfile("model.f3d")
document = archive.read_document()
if document.manifest is not None:
    print(document.manifest.name)
report = archive.verify()
print(report.status, report.members_verified)
```

```bash
fusion-kit inspect model.f3d --document --json
fusion-kit inspect assembly.f3z --package --json
fusion-kit verify assembly.f3z --json
```

F3Zの検証は内側のF3Dも対象にします。外側のZIPだけを検証する場合はCLIの `--outer-only` またはPython APIの `verify(recursive=False)` を使います。 [Python API・CLI・JSONの仕様](docs/api.md) · [Python利用例](examples/inspect_fusion.py)

## 対応範囲

読み取り専用です。形状やB-Rep、部品の配置、設計履歴、STEP・GLB変換は未対応です。F3Zで取得する文書参照は設計間の依存を表します。アーカイブを開く操作ではZIP索引を読み取り、全メンバーのCRCやサイズは `verify()` で検証します。

## ライセンス

fusion-kitのコードはMITライセンスです。依存ライブラリのライセンスと形式資料の出典を同梱しています。 [MITライセンス](LICENSE) · [第三者通知](THIRD_PARTY_NOTICES.txt) · [Rust標準ライブラリの通知](RUST_STDLIB_COPYRIGHT.html)
