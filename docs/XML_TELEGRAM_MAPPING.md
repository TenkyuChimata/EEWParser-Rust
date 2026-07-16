# JMA EEW XML →コード電文マッピング

この文書は `src/xml.rs` と `src/telegram_encoder.rs` が実装するマッピングの根拠を記録する。ページ表記は PDF 内の印刷ページ（例: `Ⅱ.21-3`）を優先し、印刷ページがない資料は PDF ページを用いる。

## 適用資料と改訂

| 資料 | 適用内容 | 日付・適用範囲 |
|---|---|---|
| `no40201.pdf` p.1–2 | EEW の提供形式、VXSE43/44/45 とコード電文の位置付け | 2020-03-31、2023-02-01 改訂 |
| `0101-0185.pdf` `Ⅰ.共通-1`、`Ⅰ.別02-1`、`Ⅰ.別03-1`、`Ⅱ.21-1`–`Ⅱ.21-18` | JMAXML 共通ヘッダ、EventID、取消、EEW Body、震源・震度・長周期・PLUM | 2025 年版（PDF 更新 2025-12-16） |
| `566.pdf` p.1–2、別添1、別添2 `Ⅱ.21` | VXSE45、長周期地震動階級、VXSE43 定義変更 | 2021-07-30 公表、令和4年度後半の運用開始予定。コード電文形式は変更なし |
| `567.pdf` p.1–3、表1 | 短縮用震央地名 `ReduceCode` の追加 | 2021-07-30 公表、566 と同時開始予定。電文形式変更なし |
| `653.pdf` p.1 | 地盤増幅度変更 | 2025-07-24 12:00 頃実施。発表形態・電文形式変更なし |
| 気象庁「緊急地震速報に関する情報のコード電文解説資料」現行版 `基本-3`–`基本-4`、`地震-3`–`地震-29` | 35/36/37/39/47/48、ND/NCN/NCPN、RK/RT/RC/EBI、取消値 | 実装時に気象庁仕様ページから取得した現行版 |
| 気象庁「緊急地震速報の概要や処理手法に関する技術的参考資料」表3 | 発表パターン 1/2/3 とコード 35/36/37 | 現行公開版 |

566・567 は開始時期を「令和4年度後半」としていた。`no40201.pdf` は 2023-02-01 改訂済みで VXSE45 を現行形式として掲載するため、本実装は VXSE45/長周期要素を有効な現行入力として扱う。653 は計算用パラメータだけの変更なのでフィールドマッピングには影響しない。

実装時に照合した気象庁の公開資料は、[配信資料に関する仕様一覧](https://www.data.jma.go.jp/suishin/shiyou/index.html)、[コード電文解説資料](https://www.data.jma.go.jp/suishin/catalogue/format/SvdEto_KinkyuJishinKeiho.2_format.pdf)、[技術的参考資料](https://ds.data.jma.go.jp/eew/data/nc/katsuyou/reference.pdf) である。

## 電文種別の選択

| XML 条件 | Telegram | 根拠・備考 |
|---|---:|---|
| 予報、`Earthquake/OriginTime` なし | 35 | 100 gal 等の「非常に強い揺れを検知・最大予測震度のみ」。XML `Ⅱ.21-1`、技術参考資料 表3 |
| 予報、`Earthquake/Condition=仮定震源要素` | 36 | PLUM 法だけが有効な仮定震源。XML `Ⅱ.21-1`、コード資料 `地震-3` |
| 予報、`Accuracy/Epicenter/@rank` が 1 または 2 | 36 | IPF 法 1/2 点。XML `Ⅱ.21-3`、技術参考資料 表3 |
| 予報、`Accuracy/Epicenter/@rank` が 3–8 | 37 | IPF 法 3 点以上等。rank 0 は一意選択不能なので error。技術参考資料 表3 |
| 予報の取消 | 39 に相当 | 単独 XML から必須時刻を復元できないため Telegram は生成せず `Cancellation(Forecast)` |
| 警報 | 47 | VXSE43 の警報情報。`no40201.pdf` p.2、コード資料 `地震-23` |
| 警報の取消 | 48 に相当 | 単独 XML から必須時刻・震源を復元できないため Telegram は生成せず `Cancellation(Warning)` |

35/36/37 は XML の意味から選択し、37 に固定しない。最終報は NCN の状態に反映する。XML の訂正は現行 NCN/NCPN に一意な対応状態がないため unsupported error とする。
取消は encoder の入力にせず library の構造化制御イベントとして返す。タイトルから予報/警報を安全に決められない EEW 取消は `Cancellation(Unknown)` とし、いずれも `Telegram=None` とする。

## 共通部・識別部

| XML パス / 値 | Telegram フィールド | 欠落時 | 適用型 | 資料 |
|---|---|---|---|---|
| `/Report/Control/EditorialOffice` | `bb` Section (`01`–`06`) | 対応官署がなければ error | 35/36/37/39/47/48 | コード資料 `基本-3` |
| `/Report/Control/Status=通常/訓練` | `nn=00/01`、取消なら `10/11` | `試験` は一意変換不能のため unsupported | 全型 | `no40201.pdf` p.2、コード資料 `基本-4` |
| `/Report/Control/DateTime` | 発信時刻 `YYMMDDhhmmss` | 必須。RFC 3339 を解釈し JST に変換 | 全型 | XML `Ⅰ.共通-1`、コード資料 `基本-4` |
| 固定値 | `C11` | なし | 全型 | 1 通の論理電文。コード資料 `基本-4` |
| `/Report/Head/EventID` | `ND` + 14 桁 | 必須 | 35/36/37/39/47/48 | XML `Ⅰ.別02-1`、コード資料 `地震-3`/`地震-23` |
| `/Report/Head/Serial` | 予報 `NCN?nn`（1–359）、警報 `NCPNnn`（1–99） | 各フィールド範囲外は error | 35/36/37/47 | XML 共通ヘッダ、コード資料 `地震-3`/`地震-23` |
| `/Report/Head/InfoType=発表` | NCN 状態 `0` | 必須 | 35/36/37 | コード資料 `地震-3` |
| `/Report/Head/InfoType=訂正` | 生成しない | 現行 NCN は `0`,`9`,`/` のみなので unsupported error | 全型 | XML 共通ヘッダ、コード資料 `地震-3` |
| `/Report/Body/NextAdvisory` が最終報を示す | NCN 状態 `9` | 通常続報は `0` | 35/36/37 | XML `Ⅱ.21-18`、コード資料 `地震-3` |
| XML に存在しない部内値 | `JD////////////// JN///` | 仕様の未設定値 | 35/36/37、47 内 NCP | コード資料 `地震-3` |
| VXSE43 内に存在しないネスト側の予報番号 | `NCN0//`、最終は `NCN9//` | 通常/最終状態は保持し、番号だけ仕様の未設定値 `//` | 47 内 NCP | コード資料 `地震-3` |

`EventID` は気象庁システムが処理を開始した時刻を基に割り当てる識別番号であり、物理的な発震時刻ではない（`0101-0185.pdf` `Ⅰ.別02-1`）。したがって時刻の代用品にはしない。

## 震源・精度・予測

| XML パス / 値 | Telegram フィールド | 欠落・未知時 | 適用型 | 資料 |
|---|---|---|---|---|
| `Earthquake/OriginTime`、35 は `ArrivalTime` | `yoyo...` 地震発生/検知時刻 | 非取消では必須 | 35/36/37/47 NCP | XML `Ⅱ.21-1`、コード資料 `地震-3` |
| `Hypocenter/Area/Code` | `kkk` 震央地名 | 必須 | 35/36/37 | XML `Ⅱ.21-2`、コード資料 `地震-4` |
| `Hypocenter/Area/ReduceCode` | `cccc` 短縮震央 | 警報では必須 | 47/48 | `567.pdf` p.1–3、コード資料 `地震-23` |
| `jmx_eb:Coordinate` | `N/Sddd E/Wdddd hhh` | 深さなしは `///`。緯度経度なしは合法電文を作れないため error | 35/36/37/47 | XML `Ⅱ.21-2`、コード資料 `地震-4` |
| `jmx_eb:Magnitude=NaN` | `mm=//` | 公式不明値 `//` | 36/37 | XML `Ⅱ.21-4`、コード資料 `地震-4` |
| 35 の Magnitude | `mm=//` | 固定 | 35 | コード資料 `地震-4` |
| `Accuracy/Epicenter/@rank,@rank2`、`Depth/@rank`、`MagnitudeCalculation/@rank`、`NumberOfMagnitudeCalculation` | `RKn1n2n3n4n5` | XML の値が 0 の場合のみ `/`、欠落は error | 35/36/37/47 NCP | XML `Ⅱ.21-3`、コード資料 `地震-5`–`地震-7` |
| `Area/LandOrSea` | `RTn1` (`0` 内陸、`1` 海域) | 公式未設定 `/` | 35/36/37/47 NCP | XML `Ⅱ.21-2`、コード資料 `地震-7` |
| Area `Category/Kind/Code` の先頭 | `RTn2` と EBI `y1` | 未対応コードは error | 35/36/37/47 NCP | XML `Ⅱ.21-10`、コード資料 `地震-7`,`地震-9` |
| `Earthquake/Condition=仮定震源要素` | `RTn3=9` | それ以外は `/` | 36/47 NCP | XML `Ⅱ.21-1`、コード資料 `地震-7` |
| `Forecast/Appendix/MaxIntChange` | `RCn1` | `/` | 35/36/37/47 NCP | XML `Ⅱ.21-8`、コード資料 `地震-8` |
| `Forecast/Appendix/MaxIntChangeReason` | `RCn2`、47 の追加理由 `PRC n4` | `/`。警報区域追加時に理由なしなら error | 35/36/37/47 | XML `Ⅱ.21-8`、コード資料 `地震-8`,`地震-25` |
| `Forecast/ForecastInt/From` | 最大震度 `ss` | XML にない場合 `//` | 36/37/47 NCP | XML `Ⅱ.21-6`、コード資料 `地震-4` |
| 35 の最大震度 | `ss=5-` | 固定 | 35 | コード資料 `地震-4` |

座標は ISO 6709 の符号と小数を数値として解釈し、0.1 度へ正確に表せない値は丸めず error にする。深さ 700 km 未満は XML 仕様の 10,000 m 精度、`+0` はごく浅い特殊値、`-700000` は 700 km 以上の特殊値として扱う。時刻は RFC 3339 の offset を解釈して JST へ変換する。

## EBI と警報地域

| XML パス / 値 | Telegram フィールド | 欠落・未知時 | 適用型 | 資料 |
|---|---|---|---|---|
| `Forecast/Pref/Area/Code` | EBI `fff` | 必須 3 桁 | 36/37/47 NCP | XML `Ⅱ.21-9`、コード資料 `地震-9` |
| Area `ForecastInt/From,To` | `Se1e2e3e4` | `不明` は `//`。不正範囲は error | 36/37/47 NCP | XML `Ⅱ.21-11`、コード資料 `地震-9` |
| Area `ArrivalTime` | EBI `hhmmss` (JST) | 到達予測型では必須 | 36/37/47 NCP | XML `Ⅱ.21-13`、コード資料 `地震-9` |
| Area `Condition=既に主要動到達と推測` | `hhmmss=//////`、`y2=1` | Condition 不一致は error | 36/37/47 NCP | XML `Ⅱ.21-13`、コード資料 `地震-9` |
| Area Kind code 末尾 `9` (PLUM) | `y2=9`、XML ArrivalTime を PLUM 予測時刻として使用 | 必須時刻がなければ error | 36/37/47 NCP | XML `Ⅱ.21-10`–`13`、コード資料 `地震-9`,`地震-28` |
| `Head/Headline/Information` の `Kind=31` と各 Area code | `PAI/PPI/PBI` 現在地域、`CAI/CPI/CBI` 追加地域 | 現在の3階層が不足すれば error | 47 | XML `Ⅰ.共通-5`–`8`、コード資料 `地震-24`–`27` |

EBI は予測震度 4 以上だけを出力し、3 地域ごとに折り返す。複数行でも decoder は `9999=` まで全 EBI を読む。

## 長周期地震動と取消の非可逆項目

| XML 要素 | コード電文での扱い | 理由 |
|---|---|---|
| `ForecastLgInt`（全国・地域） | 値域を解析・検証するが Telegram には出力しない | 566 p.2 は既存コード電文の形式を変更しないと明記し、35/36/37/47 のフィールドに長周期階級欄がない |
| `MaxLgIntChange` | 値を解析・検証するが Telegram には出力しない | 同上 |
| 長周期だけで対象となり予測震度 4 未満の Area | 47 の警報地域リストには残るが EBI には出力しない | EBI は震度 4 以上の地域を対象とする（コード資料 `地震-9`）。架空の震度を入れない |
| 取消 XML の `Earthquake` 全体 | `EewOutcome::Cancellation`、Telegram は未生成 | XML 取消 Body は Text のみ（`0101-0185.pdf` `Ⅰ.別03-1`,`Ⅱ.21-1`）。コードの地震発生/検知時刻には未設定値が定義されず、EventID も物理時刻ではない |

取消以外で仕様が定義する未知値は `/` を使う。仕様が未知値を定義していない必須項目は、見かけ上もっともらしい値を作らず error にする。
