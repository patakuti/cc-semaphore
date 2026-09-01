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
  ウィンドウ / Windowsシステムトレイ(トレイ数字のラスタライズ・割り込み点滅・
  右クリックメニュー・クリックポップアップ、いずれも実装済み)
- `extensions/cc-semaphore@patakuti/` — GNOME Shell 46 拡張
  (トップバーに状態別カウント表示、クリックでセッション一覧ポップアップ、
  daemon生死表示)
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

daemonが動作していない(heartbeatが15秒以上更新されていない)ときは、
トップバーのカウント表示が`⚠`単独表示に切り替わり、ポップアップメニューにも
「daemon停止中」の旨が表示される(Tauri版と同じ`heartbeat.json`を参照)。

## Always-on-top透過ウィンドウの起動(開発用)

```sh
cc-semaphored daemon &      # 別途、常駐デーモンを起動しておく
cargo run -p cc-semaphore-desktop
```

npm・Node.jsは使わない。`ui/` の静的アセットをそのまま `frontendDist` として
Tauriに読み込ませている。透過ウィンドウ(パネル)は既定で非表示。トレイの
右クリックメニュー「Show panel」で表示をトグルする。daemonが動作していない
(heartbeatが15秒以上更新されていない)ときは、トレイがグレーの`?`アイコンに
なり、パネルには「⚠ daemon not running」と表示される。

## 開発状況

設計・計画フェーズ完了。実装は Phase 6(Windowsシステムトレイ)まで完了し、
**Ubuntu環境で実用可能な状態**になった。実機(GNOME Shell 46 / X11、
AppIndicator拡張が有効な環境)で、GNOME拡張のパネル表示・ポップアップ、
透過ウィンドウの透過・最前面固定・実データ表示、トレイアイコンの描画・
ローテーション・右クリックメニュー・daemon生死表示まで動作確認済み。
daemon生死表示はGNOME拡張側にも反映済み(このUbuntu機で実機確認済み)。
ユーザーによるWindows実機での動作確認も完了しており、そこで得たフィードバック
(トレイ数字のサイズ・点滅の見た目・赤文字の視認性・パネル既定非表示・
daemon生死表示)はすべて反映済み。ツールチップとクリックポップアップは
Tauri本体の仕様によりLinuxでは検証不能なため、この2点のみ引き続きWindows
実機での最終確認が必要。

Windows+WSL1側の実測(Phase 0-C)は2台の実機WSL1機で完了しており、daemonは
Ubuntu機でmuslクロスビルドした静的バイナリをそのままWSL1に配布できることを
確認済み。`cc-semaphore-desktop` のWindows向けビルドは
GitHub Actions(`.github/workflows/windows-build.yml`、PRのpush毎+手動実行)
で行い、実機ユーザーはビルド成果物(.exe)をartifactからダウンロードする。
次はPhase 7(WSL1側の自動起動・パッケージング・仕上げ)。
詳細は開発時のみ手元に置く設計・計画ドキュメント(Git管理外)を参照。
