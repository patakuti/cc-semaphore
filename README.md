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
  (inotify監視 / WSL1ポーリング / スナップショット書き出し / CLI)
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

## cc-semaphored の使い方

```sh
cc-semaphored once             # 1回スキャンしてスナップショットJSONをstdoutへ
cc-semaphored watch            # 端末にライブ表示(1秒ごとに再描画)
cc-semaphored daemon           # 常駐開始(通常はsystemdから起動する)
cc-semaphored install-service  # systemd user unit を書き出す
```

`daemon` はスナップショットを `$XDG_RUNTIME_DIR/cc-semaphore/state.json`
(無ければ `~/.cache/cc-semaphore/state.json`)に書き出す。設定ファイルは
`~/.config/cc-semaphore/config.json`(任意)。詳細は `docs/protocol.md`。

## 開発状況

設計・計画フェーズ完了。実装は Phase 2(backendデーモン)まで完了。
Ubuntu環境では `cc-semaphored` が実データで動作確認済み。
次はPhase 3(GNOME Shell拡張)。
詳細は開発時のみ手元に置く設計・計画ドキュメント(Git管理外)を参照。
