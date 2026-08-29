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

## 一覧に含める条件

1. Claude Code のセッションファイル名が `^\d+\.json$`
2. JSONとしてパースできる
3. `kind === "interactive"`(既定。設定で変更可能)
4. プロセスが生存している(`pid` と `procStart` の一致で判定)
5. `status` が `busy` / `shell` / `idle` / `waiting` のいずれか
   (未知の値は表示せず除外する)

## 並び順

`waiting` → `idle` → `running` の順。同一状態内では `since` の昇順
(その状態が長く続いているものが上)。frontend側では並べ替えない。

## 書き込みの原子性

backend は `state.json.tmp` に書いてから `rename()` で置換する。
読み手はパース失敗に耐えること(50ms後に1回だけ再読込し、それでも
失敗したら前回の内容を保持する)。
