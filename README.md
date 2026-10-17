# EEWParser-Rust

気象庁（JMA）の**緊急地震速報（EEW）**を解析する、Rust 製のライブラリ・CLI ツールです。
JMA XML とコード電文（Telegram）の両方に対応し、解析結果を共通の `EewInformation` 型で扱えます。

地震情報の取得・配信サービス、監視システム、データ分析ツールなどに組み込むことを想定しています。解析処理はオフラインで動作し、ネットワーク接続や外部サービスを必要としません。

## 特長

- **XML・コード電文を共通の形式で取得** — `parse_auto` による入力形式の自動判別にも対応
- **情報の欠落を抑えた XML 解析** — 長周期地震動階級（LPGM）、予測震度、警報対象地域、精度情報、本文などを保持
- **最終報・取消報・訂正報に対応** — 情報の種類とイベントの状態を区別して扱える設計
- **不明値を安全に表現** — 未知の震源・深さ・マグニチュードを `0` として誤認しない
- **イベント追跡機能** — 続報の順序、重複した取消報、遅れて到着した電文などを扱うための `EventTracker` を提供
- **CLI と Rust API の両方を利用可能** — 解析結果を JSON として確認したり、アプリケーションに組み込んだりできます

```text
JMA XML ───────────┐
                   ├── EewInformation ── アプリケーション / JSON
JMA コード電文 ─────┘
```

## 対応形式

| 形式 | 対応する情報 |
| --- | --- |
| JMA XML | VXSE42（配信試験）、VXSE43（警報）、VXSE44（予報）、VXSE45（地震動予報）、VXSE47（工学的基盤面のリアルタイム震度） |
| コード電文 | 35、36、37、38、39、47、48、61 |

XML の `InfoKindVersion` は `1.0_0`、`1.1_0`、`1.2_0` に対応しています。入力形式ごとに元データが持つ情報は異なるため、すべての項目が常に得られるわけではありません。

## クイックスタート

### CLI で解析する

Rust / Cargo が利用できる環境で、リポジトリのルートディレクトリから実行します。

```bash
# リリースビルド
cargo build --release

# ファイルを解析（形式は自動判別）
cargo run -- -p data.txt

# XML を明示して解析
cargo run -- --input-format xml -p sample.xml

# 標準入力から解析
cat sample.xml | cargo run -- -s
```

`--input-format` には `auto`、`xml`、`telegram` を指定できます。通常報・最終報・取消報はいずれも JSON として出力できます。

### Rust アプリケーションで利用する

```rust
use eew_parser::{parse_auto, EewError, EewInformation};

fn parse_message(input: &[u8]) -> Result<EewInformation, EewError> {
    let info = parse_auto(input)?;

    // 共通の情報を取得
    let _event_id = info.event_id();
    let _serial = info.serial();
    let _message_time = info.message_time();

    // 最終報・取消報・警報を区別
    let _is_last = info.is_last_info();
    let _is_final = info.is_final();
    let _is_warning = info.is_warning(); // Option<bool>
    let _is_canceled = info.body.is_canceled;

    Ok(info)
}
```

入力形式が分かっている場合は `parse_xml(input)` または `parse_telegram(input)` も利用できます。いずれも `Result<EewInformation, EewError>` を返します。

## JSON 出力例

以下は取消報を簡略化した例です。実際の出力では、利用可能なヘッダー情報や元電文を含む `source` 情報なども出力されます。

```json
{
  "eventId": "20261016123810",
  "serialNo": "37",
  "status": "通常",
  "infoType": "取消",
  "product": "TelegramForecast",
  "body": {
    "isLastInfo": true,
    "isCanceled": true
  },
  "source": {
    "format": "telegram",
    "sentAt": "2026-10-16T12:48:50+09:00"
  }
}
```

### 最終報と取消報の違い

`isLastInfo`、`isCanceled`、`is_final()` はそれぞれ異なる意味を持ちます。

| 項目 | 意味 |
| --- | --- |
| `body.isLastInfo` | 最終報または取消報など、その情報が最後の更新に該当するか |
| `body.isCanceled` | 今回の電文が取消報か |
| `is_final()` | 元の電文で最終報として明示されているか |
| `body.isWarning` | 警報に該当するか。判定できない場合や対象外の場合は省略 |

**最終報を受信した後でも取消報が届く場合があります。** そのため、`isLastInfo: true` を「以後の電文を一切受け付けない」という意味で扱わないでください。また、取消報に震源情報や予測震度が含まれない場合、それらを前回の情報で補完することはありません。

## イベントの追跡

`EventTracker` は、同じ地震について連続して届く情報の管理を補助するオプション機能です。

```rust
use eew_parser::{parse_auto, EewError, EventTracker};

fn process(tracker: &mut EventTracker, input: &[u8]) -> Result<(), EewError> {
    let info = parse_auto(input)?;
    let decision = tracker.observe(&info);

    // decision に応じた保存・通知・配信はアプリケーション側で実装
    let _ = decision;
    Ok(())
}
```

イベント ID だけでなく、情報の種類や通常・訓練・試験の区別も考慮して状態を管理します。例えば、**第37報 → 第42報（最終報）→ 第37報（取消報）** のように、取消報の番号が直前の通常報より小さくても適切に扱えます。

なお、`EventTracker` はメモリ上の追跡機能です。状態の永続化、保存期間、通知・配信ポリシーは利用するアプリケーションで管理してください。

## XML とコード電文について

XML は元の構造を直接解析し、ヘッダー、警報対象地域、LPGM、震度予測、自由文などをできる限り保持します。元の入力は `source.raw` から参照できます。

一方、コード電文には XML 固有の情報が含まれていないため、存在しない項目を推測して補完することはありません。日時も入力形式によって意味が異なり、コード電文の送信時刻は XML の `reportDateTime` と必ずしも一致しません。

## 既存バージョンからの移行

現在の API では `parse_auto`、`parse_xml`、`parse_telegram` の戻り値を `EewInformation` に統一しています。

- 従来の `EewOutcome` と `CancellationEvent` は廃止されました。取消報も `EewInformation` として扱います。
- 従来のコード電文向け表示モデルが必要な場合は `decode_telegram_legacy` を使用できます。
- `encode_xml_as_telegram` は**明示的な情報欠落を伴う変換**です。元の JMA コード電文を復元する機能ではありません。
- 過去の NCN 6 / 7 / 8 の解釈が必要な場合は、`parse_telegram_with_convention` と `NcnConvention::Legacy2013` で明示的に指定してください。

## 制限事項

- WMO / STX などの外側の通信フレーム、文字コード変換、分割電文の再構成には対応していません。解析には完全な論理電文が必要です。
- 未知の XML バージョンや、解釈を一意に決定できない複数の震源・マグニチュード情報などは、誤った値を返す代わりにエラーとします。
- XML の未知の拡張要素は元データとして保持できますが、すべてが専用の構造化フィールドになるわけではありません。
- コード電文のみからは復元できない日時や情報があり、不明な数値を `0` に置き換えることはしません。
- 入力は 16 MiB、XML 要素の入れ子は 128 階層までです。完全な XSD 検証や外部サービスからの電文取得は行いません。

## 開発・テスト

```bash
cargo fmt --all -- --check
cargo check --all-targets --all-features
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

## 参考仕様

- [気象庁 防災情報 XML](https://xml.kishou.go.jp/)
- [DMDATA — 緊急地震速報 eew-information](https://dmdata.jp/docs/reference/conversion/json/schema/eew-information/)

> [!NOTE]
> 本プロジェクトは緊急地震速報の電文を解析するためのソフトウェアです。解析結果の利用・配信にあたっては、情報の発表元、更新状況、欠落・遅延の可能性を考慮してください。
