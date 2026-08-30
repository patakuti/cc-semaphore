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
- `extensions/cc-semaphore@patakuti/` — GNOME Shell 46 拡張
  (トップバーに状態別カウント表示、クリックでセッション一覧ポップアップ)
- `ui/` — GNOME拡張以外のフロントエンドが共有する静的UIアセット
  (ビルド不要。npm/Node.jsは使用しない)。`demo.html` をブラウザで開けば
  `python3 -m http.server -d ui` でスタンドアロン確認できる
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

## GNOME Shell拡張のインストール(開発用)

```sh
ln -sfn "$(pwd)/extensions/cc-semaphore@patakuti" \
  ~/.local/share/gnome-shell/extensions/cc-semaphore@patakuti
# GNOME Shellをリスタート(X11: Alt+F2 → r → Enter。Waylandはログアウト/ログイン)
gnome-extensions enable cc-semaphore@patakuti
```

## 開発状況

設計・計画フェーズ完了。実装は Phase 4(共有UIアセット)まで完了し、
**Ubuntu環境で実用可能な状態**になった。実機(GNOME Shell 46 / X11)で
パネル表示・ポップアップとも動作確認済み。Windows+WSL1側の実測(Phase 0-C)も
2台の実機WSL1機で完了しており、daemonはUbuntu機でmuslクロスビルドした
静的バイナリをそのままWSL1に配布できることを確認済み。次はPhase 5〜7
(TauriによるAlways-on-top透過ウィンドウ・Windowsトレイ・仕上げ、いずれも
Windows側でビルド)。
詳細は開発時のみ手元に置く設計・計画ドキュメント(Git管理外)を参照。
