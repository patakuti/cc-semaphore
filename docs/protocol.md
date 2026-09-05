# cc-semaphore スナップショットプロトコル

backend (`cc-semaphored`) と frontend (GNOME拡張 / Tauriアプリ) の唯一の接点。
詳細な設計判断の根拠は `02_design.md` §2 を参照。

## 保存場所

| 環境 | パス |
|---|---|
| Ubuntu / WSL1 ローカル | `$XDG_RUNTIME_DIR/cc-semaphore/state.json`(無ければ `~/.cache/cc-semaphore/state.json`) |
| WSL1 → Windows 側 | `/mnt/c/Users/<user>/AppData/Local/cc-semaphore/state.json`(設定で上書き可) |

## フォーマット (`version: 1`)

```json
{
  "version": 1,
  "generatedAt": 1787972905123,
  "host": "ubuntu-box",
  "counts": { "running": 2, "waiting": 1, "idle": 3 },
  "sessions": [
    {
      "id": "f12359a7-594c-4d3a-8bb2-2ef7538cc250",
      "pid": 2071740,
      "name": "cc-semaphore-b7",
      "cwd": "/home/patakuti/CCROOT/cc-semaphore",
      "state": "waiting",
      "rawStatus": "waiting",
      "waitingFor": "input needed",
      "since": 1787972418996,
      "startedAt": 1787972399274
    }
  ]
}
```

- `since`: Claude Code の `statusUpdatedAt` をそのまま格納した epoch ms。
  経過時間は `now - since` として毎秒レンダリング時に算出する
  (状態ファイルにハートビートが無いため、これが唯一の正確な情報源)。
- `waitingFor`: `state === "waiting"` のときのみ存在。
- `counts`: `sessions` から算出できる冗長フィールドだが、頻繁な集計読み取り
  (GNOME拡張のパネル表示)のためにあらかじめ含める。
- 時刻はすべて同一マシンのepoch ms。

## バージョニングポリシー

`version`は破壊的変更(フィールドの削除・意味変更)をしたときだけ上げる。
フィールド追加のみの変更では上げない。backendとfrontendは別バイナリ・
別リポジトリ的に独立して更新されうるため(WSL越しの`cc-semaphored`と
`cc-semaphore-desktop`、GNOME拡張はdaemonのバイナリ更新とは独立に更新
される)、各frontendは読み込んだスナップショットの`version`が自分の
対応バージョンと一致するか確認し、一致しない場合は黙って壊れたデータ
や直前のデータを表示し続けるのではなく、明示的な警告(`⚠ unsupported
snapshot version`)を表示する。daemonがダウンしている場合の`⚠ daemon
not running`とは別扱いだが、UI上の見た目・優先度は同等(daemon-downが
優先)。実装は`cc-semaphore-desktop`の`SnapshotEvent.versionSupported`と
GNOME拡張の`SUPPORTED_SNAPSHOT_VERSION`定数を参照(02_design.md §2.2.1)。

## 一覧に含める条件

1. Claude Code のセッションファイル名が `^\d+\.json$`
2. JSONとしてパースできる
3. `kind === "interactive"`(既定。設定で変更可能)
4. プロセスが生存している(`pid` と `procStart` の一致で判定)
5. `status` が `busy` / `shell` / `idle` / `waiting` のいずれか
   (未知の値は表示せず除外する)

## 並び順

`waiting` → `idle` → `running` の順。同一状態内では `since` の昇順
(その状態が長く続いているものが上)。これはbackendが生成する
スナップショット自体の順序であり、APIコントラクトとして維持する。

ただし、実際にセッション一覧を表示する2つのUI
(cc-semaphore-desktopのAlways-on-top透過ウィンドウ、GNOME拡張の
ポップアップ)は、いずれも表示直前にこの順序を無視し**経過時間昇順**
(直近で状態が変わったものが上)へ独自に並べ替える(表示専用のローカルな
挙動で、上記のスナップショット自体は変更しない。詳細は02_design.md
§7.2、GNOME拡張側は`extension.js`の`_renderMenu`)。cc-semaphore-desktop
にはかつてトレイの左クリックで開く別のポップアップ(`popup.html`)が
あり、そちらは元のbackend順のままだったが、2026-09-05にそのポップアップ
自体を廃止したため、この例外は残る2つのUIの共通ルールになった。

## 書き込みの原子性

backend は `state.json.tmp` に書いてから `rename()` で置換する。
読み手はパース失敗に耐えること(50ms後に1回だけ再読込し、それでも
失敗したら前回の内容を保持する)。
