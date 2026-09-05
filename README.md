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

Rustツールチェーンなしで導入したい場合は、GitHub Actions
(`.github/workflows/linux-build.yml`、PRのpush毎+手動実行)がビルドする
musl静的バイナリをartifactからダウンロードできる(ネイティブUbuntu・
WSL1共通)。**推奨インストール先は`~/.local/bin/cc-semaphored`**
(`install-wsl1-autostart`は実行時のバイナリパスをそのままフックに
埋め込むため、後で場所を変えると自動起動が壊れる):

```sh
mkdir -p ~/.local/bin
mv cc-semaphored ~/.local/bin/
chmod +x ~/.local/bin/cc-semaphored
```

```sh
cc-semaphored once                    # 1回スキャンしてスナップショットJSONをstdoutへ
cc-semaphored watch                   # 端末にライブ表示(1秒ごとに再描画)
cc-semaphored daemon                  # 常駐開始
cc-semaphored install-service         # (ネイティブLinux) systemd user unit を書き出す
cc-semaphored install-wsl1-autostart  # (WSL1) ~/.bashrcに自動起動フックを追加する
cc-semaphored install-wsl1-autostart --print  # 追加せず、内容を表示するだけ
```

`daemon` はスナップショットを `$XDG_RUNTIME_DIR/cc-semaphore/state.json`
(無ければ `~/.cache/cc-semaphore/state.json`)に書き出す。スナップショット
自体のJSON形式は `docs/protocol.md` を参照。

常駐化はOSごとに方法が異なる。ネイティブLinuxは`install-service`で
systemd user unitを導入すればログイン後は自動で動く。WSL1にはsystemdも
「OS起動」に相当するものも無いため、`install-wsl1-autostart`で
`~/.bashrc`にフックを追加する — 以降、シェルを開くたびに起動を試み、
既に動いていれば(ロックファイルにより)何もしない。`~/.bashrc`を
自動編集されたくない場合は`--print`を付けると、ファイルには触れず
追記すべき内容を表示するだけになる(`.zshrc`等への貼り付けも自分で行う)。

設定ファイル(`~/.config/cc-semaphore/config.json`、任意・省略可)で
上書きできるパラメータ:

| キー | 既定値 | 意味 |
|---|---|---|
| `includeKinds` | `["interactive"]` | 表示対象とするセッション種別 |
| `inotifyDebounceMs` | `150` | Linux inotifyイベントのデバウンス |
| `livenessTickSecs` | `5` | Linuxでのプロセス生存確認の周期 |
| `wsl1PollIntervalMs` | `1000` | WSL1でのポーリング間隔 |
| `windowsStateDir` | (自動判定) | WSL1→Windows書き出し先の明示的な上書き |

`cc-semaphore-desktop`(Windows/Tauri側)も同じ`config.json`
(Windowsは`%APPDATA%\cc-semaphore\config.json`)から`windowsPollIntervalMs`
(既定`500`、DrvFs越しのmtimeポーリング間隔)だけを読む。上記以外の
タイミング(UI再描画・トレイのローテーション・点滅・GNOME拡張の保険
再読込)は意図的な固定値で、設定ファイルでは変更できない。

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
cargo run -p cc-semaphore-desktop           # トレイ経由(既定で非表示)
cargo run -p cc-semaphore-desktop -- --panel  # 起動直後からパネルを表示
```

npm・Node.jsは使わない。`ui/` の静的アセットをそのまま `frontendDist` として
Tauriに読み込ませている。

透過ウィンドウ(パネル)は既定で非表示。トレイの右クリックメニュー
「Show panel」で表示をトグルするか、`--panel`オプション付きで起動すると
トレイ操作なしに起動直後からパネルが開く(トレイ自体は通常通り起動し、
終了は引き続きトレイメニューの`Quit`から行う)。

パネルは既定で`running/waiting/idle`の3つの数字(丸い不透明バッジ)だけを
表示し、クリックするとセッション一覧に展開する。展開時の一覧は、
backendが返す`waiting→idle→running`優先度ではなく**経過時間が短い順**
(直近で状態が変わったものが上)に並ぶ、このウィンドウ独自の表示順。
カードの背景は既定で90%透過。展開中に`[`(より透明に)/`]`(より不透明に)
キーで5%刻みに調整でき、値は次回起動時にも引き継がれる。

daemonが動作していない(heartbeatが15秒以上更新されていない)ときは、
トレイがグレーの`?`アイコンになり、パネルには「⚠ daemon not running」と
表示される。

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
確認済み。`cc-semaphore-desktop` のWindows向けビルドと`cc-semaphored`の
musl静的バイナリビルドは、それぞれGitHub Actions
(`.github/workflows/windows-build.yml` / `linux-build.yml`、いずれも
PRのpush毎+手動実行)で行い、実機ユーザーはビルド成果物を
artifactからダウンロードする。

現在はPhase 7(WSL1側の自動起動・パッケージング・仕上げ)の終盤。
設定パラメータの対応状況の最終確認(§10)、設計書・実装間の齟齬の
最終監査(見つかった齟齬 — 未実装だった`--reap-stale`の削除、廃止済み
機能の記述漏れ等 — はすべて解消済み)、Windows向けNSISインストーラの
生成とログイン時自動起動、WSL1側の自動起動(`install-wsl1-autostart`)
まで実装済み。Windows側は実機で動作確認済み。WSL1側は実装・ローカルの
単体テストまで完了しており、実機WSL1での最終確認待ち。
詳細は開発時のみ手元に置く設計・計画ドキュメント(Git管理外)を参照。
