# 内部資料(開発用)

InfoDeskを開発するコントリビュータ向けの内部ドキュメント。内部資料は日本語で書く。公開用ドキュメント(連番、英語が正本)は[`docs/`直下](../01-overview.md)、目次は[README](../../README.md)。

| ドキュメント | 内容 |
|---|---|
| [architecture.md](architecture.md) | システム設計書: 構成図・ファイルマップ・データフロー・ジョブとスケジューラー・チャットのサイドカー・**変更レシピ** |
| [data-model.md](data-model.md) | データ設計書: SQLiteスキーマ(`user_version` 1〜5)・テーブルと主な列・設定キー・FTS5 |
| [tech-stack.md](tech-stack.md) | 技術スタック: `Cargo.toml` / `package.json` / `Cargo.lock` から読んだバージョンと選定理由 |
| [design-system.md](design-system.md) | デザインシステム: `theme.css` のトークン・コンポーネント規約・`design.pen` と `design/exports/` |
| [feature-inventory.md](feature-inventory.md) | 機能インベントリ: 設計書の節ごとに実装済み/未実装を棚卸し |
| [testing.md](testing.md) | テスト: 各スイートの実行方法・フィクスチャ・モックバックエンドによるスクリーンショット |
| [../superpowers/specs/](../superpowers/specs/) | 設計書(マスタースペック): 設計判断の記録。節番号は feature-inventory.md が参照する |
| [../superpowers/plans/](../superpowers/plans/) | 実装プラン(日付順、Plan 1〜6とその補足): 何をどの順で作ったかの履歴 |

## 読む順番の目安

1. 全体像: [architecture.md](architecture.md) の1〜3節。
2. 保存されるデータ: [data-model.md](data-model.md)。
3. 変更したい箇所のレシピ: [architecture.md](architecture.md) の「変更レシピ」。
4. 画面を触る: [design-system.md](design-system.md)。
5. 何が未実装か: [feature-inventory.md](feature-inventory.md)。

## 前提

- 対象はmacOSのみ(Apple Siliconで確認)。バージョン0.1.0、アルファ版の個人プロジェクト。
- リモートは GitHub の [kousuke-takeuchi/InfoDesk](https://github.com/kousuke-takeuchi/InfoDesk) (既定ブランチ `master`)。リモートのビルド(CI)は無い。ローカルで `cargo test` と各 `npm test` を通すことが検証の基準([testing.md](testing.md))。
- 設計書(スペック)は2026-10-06付の1本で、追記で14〜16節まで伸びている。実装は設計書に従うが、実装で変えた点は feature-inventory.md に差分として書く。
