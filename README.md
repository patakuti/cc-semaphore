# cc-semaphore

複数の Claude Code セッションを並行して動かしているとき、それぞれが
「動作中(running)」「入力・許可待ち(waiting)」「放置中(idle)」の
どれなのかを一目で把握するための監視ツール。

対象OS: Ubuntu(ネイティブLinux) / Windows 10・11 + WSL1。

## 状態モデル

| 状態 | 意味 | 色 |
|---|---|---|
| `running` | 処理中 | 緑 |
| `waiting` | 入力・許可待ち(対応必須) | 黄 |
| `idle` | タスク完了後、放置中(急ぎではない) | 赤 |

## 構成

- `crates/cc-semaphore-core` — 状態ファイルのパース・3値マッピング・
  生存判定・スナップショット生成(Rust、共有ライブラリ)
- `crates/cc-semaphore-daemon` — 常駐デーモン `cc-semaphored`
  (Phase 2で追加予定)
- `crates/cc-semaphore-desktop` — Tauri v2 による Always-on-top 透過
  ウィンドウ / Windowsシステムトレイ(Phase 5〜6で追加予定)
- `extensions/cc-semaphore@patakuti/` — GNOME Shell 46 拡張(Phase 3で追加予定)
- `ui/` — GNOME拡張以外のフロントエンドが共有する静的UIアセット
  (ビルド不要。npm/Node.jsは使用しない)
- `docs/protocol.md` — backend/frontend間のスナップショットプロトコル仕様

## ビルド

```sh
cargo build
cargo test
```

## 開発状況

設計・計画フェーズ完了。実装は Phase 1(core クレート)まで完了。
詳細は開発時のみ手元に置く設計・計画ドキュメント(Git管理外)を参照。
