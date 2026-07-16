# EEWParser-Rust

EEWParser-Rust は、気象庁（JMA）の緊急地震速報を扱う Rust library / CLI です。入力には JMA コード電文（Telegram）と気象庁防災情報 XML を使用できます。リアルタイム処理では、子プロセスや JSON の再解析を介さず library API を同一プロセスから呼び出すことを推奨します。

XML は JSON へ直接変換しません。XML を仕様に沿ったコード電文へ再符号化し、その電文を Telegram デコーダへ渡します。

```text
JMA EEW XML -> Telegram encoder -> Telegram decoder -> EEW JSON
```

## 対応範囲

- Telegram の直接デコード: 35、36、37、38、39、47、48、61
- XML の解析: VXSE43（警報）、VXSE44（予報）、VXSE45（地震動予報）相当のタイトルと名前空間
- XML から生成する Telegram: 35、36、37、47
- XML の取消: `EewOutcome::Cancellation` として構造化して返します。公式の取消 XML にはコード電文で必須の地震発生・検知時刻などがないため、偽の 39/48 は生成しません。39/48 の Telegram 直接入力は従来どおりデコードできます。
- XML の訂正: 現行コード電文の NCN/NCPN に XML の `InfoType=訂正` と一意に対応する状態がないため、推測せず明示的なエラーにします。既存 Telegram の状態コード 6/7/8 のデコード互換性は維持します。

XML からの電文種別は次の規則で決めます。

- `35`: `OriginTime` がない「非常に強い揺れを検知・最大予測震度のみ」（100 gal）
- `36`: 仮定震源要素、または震央精度 rank 1/2（IPF 法 1/2 点）
- `37`: それ以外の予報
- `47`: 警報 XML

予報・警報の取消 XML はそれぞれ `Forecast` / `Warning` の制御イベントになります。タイトルなどから安全に区別できない EEW 取消は `Unknown` です。いずれも encoder へ渡さず、`Telegram` は `null` になります。

詳細な根拠と XPath 対応は [XML_TELEGRAM_MAPPING.md](docs/XML_TELEGRAM_MAPPING.md) を参照してください。

## ビルド

```bash
cargo build --release
```

実行ファイルは `target/release/EEWParser-Rust` に生成されます。開発時は以下の例のように `cargo run --` でも実行できます。

## Library API

crate の library 名は `eew_parser` です。公開 API はネットワーク等で受信した payload をそのまま `&[u8]` で受け取り、非 UTF-8 は明示的なエラーにします。

```rust
use eew_parser::{parse_auto, parse_telegram, parse_xml, EewError, EewOutcome};

fn parse_received(
    payload: &[u8],
    telegram_payload: &[u8],
    xml_payload: &[u8],
) -> Result<EewOutcome, EewError> {
    let _telegram_report = parse_telegram(telegram_payload)?;
    let _xml_outcome = parse_xml(xml_payload)?;
    parse_auto(payload)
}
```

戻り値は正常報の `EewOutcome::Report(DecodedEew)`、または取消の `EewOutcome::Cancellation(CancellationEvent)` です。正常 XML の `Report` は必ず次の共通経路を通ります。

```text
XML semantic parser -> Telegram encoder -> common Telegram decoder -> DecodedEew
```

`CancellationEvent` は `Type`、`EventID`、`CancellationKind`、`ReportDateTime`、`TargetDateTime`、`Title`、`InfoKind`、`Telegram` を持ちます。単独 XML から合法な 39/48 を復元できない場合、`Telegram` は `None`（JSON では `null`）です。入力の raw XML は呼び出し側が既に保持しているため、構造体内には複製しません。

## 入力

`-p <path>` はファイル全体を読み、`-s` は stdin を読みます。どちらも指定しなければ従来どおり `data.txt` を読みます。`-p` と `-s` は同時指定できません。

`--input-format` は `auto`（既定）、`telegram`、`xml` のいずれかです。`auto` は UTF-8 BOM と先頭空白を除いた最初の文字が `<` なら XML、それ以外なら Telegram と判定します。明示形式と実データが矛盾すると非ゼロで終了します。

取消制御イベントを CLI の JSON として受け取る場合は `--emit-control-events` を指定します。未指定時は従来の厳格な CLI 動作を保ち、取消 XML は説明付きエラーで非ゼロ終了します。通常報の出力形式はこのオプションで変わりません。

ファイル入力:

```bash
cargo run -- -p data.txt
cargo run -- -p sample.xml
cargo run -- --input-format xml -p sample.xml
cargo run -- --emit-control-events --input-format xml -p cancellation.xml
```

stdin の Telegram 入力:

```bash
cat data.txt | cargo run -- -s
printf '%s\n' '37 03 00 ... 9999=' | cargo run -- --input-format telegram -s
```

stdin の XML 入力:

```bash
cat sample.xml | cargo run -- -s
cat sample.xml | cargo run -- --input-format xml -s
```

stdin は、XML の場合は EOF まで、Telegram の場合は最初の完全な `=` まで読みます。したがって XML 宣言や属性に含まれる `=` で切れません。Telegram は EOF に達しても終端 `=` がなければ不完全としてエラーになります。

## 出力

既存の JSON フィールドに `Telegram: String` を追加しています。

- Telegram 入力: 入力先頭から最初の `=` まで、デコーダが実際に使用した論理電文
- XML 入力: encoder が生成し、デコーダへ実際に渡した電文

`Telegram` は空行を除き、各行の前後を取り、改行を単一スペースで連結します。定長フィールド内部の連続スペースは圧縮せず、末尾の `=` を保持します。

```json
{"CodeType":"Ｍ、最大予測震度及び主要動到達予測時刻の緊急地震速報","EqID":"20240101161010","Ebis":[],"Telegram":"37 03 00 ... 9999="}
```

実際の出力には従来の全フィールドが含まれます。

取消を opt-in した場合は次の形です。`Telegram: null` は「Telegram が空」という意味ではなく、仕様上合法な 39/48 を単独 XML から生成できないことを表します。

```json
{"Type":"Cancellation","EventID":"20230906063640","CancellationKind":"Forecast","ReportDateTime":"2023-09-06T06:37:52+09:00","TargetDateTime":"2023-09-06T06:37:52+09:00","Title":"緊急地震速報（地震動予報）","InfoKind":"緊急地震速報","Telegram":null}
```

## リアルタイム状態追跡

`EventTracker` は任意で利用できる軽量なメモリ内分類器です。グローバル状態、ロック、永続化、配信処理は持ちません。

```rust
use eew_parser::{parse_auto, EventTracker, EewOutcome};

fn handle_payload(
    tracker: &mut EventTracker,
    payload: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let outcome = parse_auto(payload)?;
    let decision = tracker.observe(&outcome);

    match &outcome {
        EewOutcome::Report(report) => {
            // decision に従って更新・配信・保存する
        }
        EewOutcome::Cancellation(cancel) => {
            // EventID で取消を配信する。existed=false でも下流へ通知する
        }
    }
    Ok(())
}
```

推奨する扱いは、`Accepted` を更新・配信、`Duplicate` を記録または重複抑止、`OlderThanLatest` を履歴記録して最新状態へは適用しない、`Final` を最終状態として配信、`Cancelled { existed }` を `existed` にかかわらず取消として配信することです。数字へ変換できない Serial は 0 に置換せず、数字による乱序比較を行いません。実行可能な全体例は [realtime_integration.rs](examples/realtime_integration.rs) にあります。

## XML 再符号化の制約

再符号化結果は JMA が最初に送信したコード電文の逐字節復元ではありません。XML に存在する意味情報から、現在のコード電文仕様に従って新しい論理電文を生成します。

- `JD`、`JN` は部内利用値を XML から復元できないため、仕様の未設定値を使用します。
- VXSE43 内の警報番号は `NCPN` に使用できますが、47 内にネストする VXSE45 側の予報番号は XML にないため、既知の通常/最終状態と未設定番号を組み合わせた `NCN0//` / `NCN9//` とします。
- `ForecastLgInt` と `MaxLgIntChange` は解析・検証しますが、技術情報第 566 号はコード電文形式を変更しないと定義しています。旧コード電文には長周期地震動階級の格納欄がないため JSON には現れません。
- 震度 4 未満で長周期地震動だけが対象となった地域は EBI に表現できません。47 の警報地域リストには保持されますが、予報ブロックの EBI からは除外します。
- XML の取消は Body が `Text` のみであり、39/48 に必要な地震時刻や震源を単独文書から復元できません。値を捏造せず `CancellationEvent` として返します。
- `Control/Status=試験` は XML の配信テストとコード 38/電文種別 20・30 の対応を単独 XML から一意に決められないため、現在は unsupported です。`通常` と `訓練` は対応します。
- `InfoType=訂正`、`Accuracy/Epicenter rank=0`、試験 XML、および公式の合法な未設定値がない必須項目は、取消に見せかけず文脈付き `EewError::Unsupported` のまま拒否します。

## テスト fixture

`tests/fixtures` にはネットワーク不要の代表例を保存しています。五つの公開サンプルディレクトリに含まれる全 VXSE45 を再取得する場合は次を実行します。

```bash
./scripts/fetch_official_fixtures.sh /tmp/eew-fixtures
```

テスト実行時にこのスクリプトやネットワークは必要ありません。
