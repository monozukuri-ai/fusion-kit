# Python API・JSON契約

v0.1はF3D / F3Zの読み取り専用APIを提供する。通常のCPython 3.11以上を対象とし、解析結果のnative viewは直接構築・継承・書換えしない。
入口は`fusion_kit.readfile(path)`と`fusion_kit.read(data)`。`Limits`だけは利用者が構築できる。

## 入力と解析scope

```python
from pathlib import Path
import fusion_kit

archive = fusion_kit.readfile(Path("assembly.f3z"), strict=False)
info = archive.read_package()
for doc in info.documents:
    print(doc.id, doc.document_guid, doc.status)
    if doc.metadata is not None and doc.metadata.manifest is not None:
        print(doc.metadata.manifest.name)
if info.root_document_id is not None:
    child = archive.open_document(info.root_document_id, strict=False)
    preview = child.get_preview(strict=False)
report = archive.verify(strict=False)
print(info.exit_code, report.exit_code)
```

実行可能な例を同梱する。 [文書とパッケージのJSON取得](../examples/inspect_fusion.py) · [文書ごとのプレビュー取得](../examples/package_members.py)

| 操作 | scopeと返却値 |
|---|---|
| `readfile(str / PathLike[str])` / `read(bytes)` | 所有するsnapshotとZIP索引。可変bufferは受け付けない |
| `archive.members` / `read_member(name)` | 外側ZIP。読取時に展開量・サイズ・CRCを検証してbytesを返す |
| `archive.read_document()` | F3Dの`DocumentInfo`。Manifest、asset / segment、未知領域 |
| `archive.read_package()` | F3Zの`PackageInfo`。root、文書依存、Redirections照合 |
| `archive.documents` | F3Dでは`list[DocumentInfo]`、F3Zでは`list[PackageDocument]` |
| `archive.references` / `root_document_id` | package参照、または対象文書ID。未対応scopeでは例外 |
| `archive.open_document(id, *, strict=None)` | F3Zのchild `Archive`。IDはpackage内のmember path |
| `archive.get_preview(*, document_id=None, strict=None)` | rootまたは指定文書の画像bytes。不在は`None`、空memberは`b""` |
| `archive.read_span(region)` | source hash・範囲・領域hashを検証して未知領域の原bytesを取得 |
| `archive.verify(*, strict=True, recursive=True)` | F3Z内archiveを含むZIP検証。`recursive=False`なら外側のみ |

`readfile` / `read`は索引までを読み、文書・packageは遅延解析する。
索引の成功はCRC・文書依存・形状の成功を意味しない。`archive.status` / `archive.issues` / `archive.to_dict()`は索引の結果を保持する。
文書の診断は`read_document().issues`、packageは`read_package().issues`、全量検証は`verify().issues`へ返す。

F3Dの文書IDはFusion GUID。F3Zの`PackageDocument.id`はmember pathで、`package_sha256`と組にして識別する。
Fusion GUIDは`document_guid`、graph内の数値IDは`graph_index`と組にして保持する。
root不明時に先頭文書を選ばず、文書数・参照数からOccurrenceやbody数を生成しない。

## strictと状態

| 設定 | 意味 |
|---|---|
| `readfile/read(strict=True)` | ZIP索引で検出した未対応entryを例外にする。metadataの解析は要求時 |
| `read_document/read_package/open_document/get_preview(strict=None)` | 入力時設定を継承。明示したboolはその操作だけに適用 |
| `documents/references/root_document_id` | 入力時strict設定を使用 |
| `verify(strict=True)` | 入力時設定とは独立して、失敗時にreport付き例外を返す |

`open_document`で生成したchildは、その操作で選んだstrict設定を継承する。親の設定とcache済み情報は変更しない。
`strict=False`でも破損、上限超過、読取不能を無条件に受理しない。参照上の矛盾は、安全に検査できた`invalid`情報として取得できる。
未解析のcollectionは`None`、解析済みの空collectionは`[]`、要求していない層は`not_requested`で区別する。

| status | exit_code | 意味 |
|---|---:|---|
| `complete` | 0 | 要求したscopeを完了 |
| `partial` | 2 | 安全に保持できる部分結果・warningあり |
| `unsupported` | 2 | 未対応schema・形式あり |
| `invalid` | 1 | 破損や整合性の失敗 |

viewと例外の`exit_code`はRustの同じ判定を使う。primary診断のseverityだけでscope全体のstatusを再判定しない。

## 例外と診断

`FusionError`を基底に`FormatError`、`UnsupportedFeatureError`、`ReferenceResolutionError`、`ResourceLimitError`を返す。
ファイルの不存在・権限はPythonの`FileNotFoundError` / `PermissionError`、未知member / 文書IDは`KeyError`を維持する。
`Limits`の設定値不正は`ValueError` / `OverflowError`。CLIではこれらもinvalid JSONへ変換する。

readerのパース・読取・検証で返す例外には次の属性がある。引数の型・Limits設定値の例外は除く。builtin例外の追加属性は必要に応じて`getattr`で取得できる。

- `code / stage / source / member / offset / offset_basis / context / message`：primary診断。
- `diagnostic`：primary診断のdict。`issues`：利用可能な全診断の読み取り専用view。
- `status / exit_code`：失敗した要求scopeの状態。strictによるpartial拒否もpartial / 2を保つ。
- `report`：全量検証の途中結果。利用できなければ`None`。
- `result`：`schema_version / status / issues / report`を持つJSONエラー表現。

`member`は実memberまたは欠落した対象名、`context`はJSON / graph内の論理位置。JSON Pointer形式とは定義しない。
inner Redirectionsの診断は`source="親!child.f3d"`、`member="RedirectionsStream.dat"`のように分離する。
`source`は表示用のsnapshot名で、永続IDにはsource SHA-256とpackage内IDを使う。

offsetは0始まり。`offset_basis="archive"`はZIP headerや圧縮payload先頭、`"member"`は展開後Manifestのcursor / 候補位置を表す。
正確な位置がなければoffsetとbasisは`None`。検出位置と実際の破損byteを同一視しない。

## JSONと型

`to_dict()`は独立したコピー、`to_json()`は同じschemaのJSON文字列を返す。返却dict / listを書き換えてもnative cacheは変化しない。
`fusion_kit.types`に`Status` / `ExitCode`等のLiteralと`ArchiveJSON` / `DocumentJSON` / `PackageJSON` / `ValidationJSON` / `ErrorJSON`等のTypedDictを提供する。
schema_versionは1。sidecarの未解釈値は再帰的な`JSONValue`、未知の参照種別は`str | None`と元JSONで保持する。
未知のgeometryやbody数を補完するfieldは提供しない。

## CLI

```bash
fusion-kit inspect model.f3d --json
fusion-kit inspect model.f3d --document --json
fusion-kit inspect assembly.f3z --package --json
fusion-kit verify assembly.f3z --json
fusion-kit verify assembly.f3z --outer-only --json
```

CLIはpartialを含む情報を表示し、上表の終了コードを返す。
`--json`では引数エラーも含め、stdoutに1つのJSONを返す。`--help` / `--version`は通常のtext出力・exit 0。
`--document`と`--package`は排他的。引数エラーは`invalid`と終了コード1を返す。
