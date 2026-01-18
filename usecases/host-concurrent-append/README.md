# Host Trait Concurrent Append Test

複数の WASM インスタンスがネイティブスレッドで並列実行され、共有 VFS (fs-core) に対して
同時追記を行い、適切なロック制御によりデータ競合が発生しないことを検証するテストです。

**RPC 版との違い**: このテストは真のマルチスレッド並列アクセスを行うため、
fs-core のロック実装（DashMap + per-inode RwLock）を実際に検証します。

## アーキテクチャ

```
┌────────────────────────────────────────────────────┐
│              host-runner (native Rust)             │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐         │
│  │ Thread 1 │  │ Thread 2 │  │ Thread 3 │         │
│  │ Store 1  │  │ Store 2  │  │ Store 3  │         │
│  │ WASM 1   │  │ WASM 2   │  │ WASM 3   │         │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘         │
│       │             │             │               │
│       ▼             ▼             ▼               │
│  ┌─────────────────────────────────────────────┐  │
│  │     fs-core (Arc<Fs>)                       │  │
│  │     - DashMap (lock-free concurrent map)    │  │
│  │     - Per-inode RwLock                      │  │
│  │     /shared/concurrent.log                  │  │
│  └─────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────┘
```

## 実行方法

```bash
# デフォルト (3 クライアント × 50 追記 = 150 行)
./run-test.sh

# カスタム (4 クライアント × 100 追記 = 400 行)
./run-test.sh 4 100

# Makefile から
make run-usecase-host-concurrent
```

## 検証内容

1. **行数の整合性**: 期待される行数 (クライアント数 × 追記回数) と一致するか
2. **フォーマットの正当性**: 各行が `[timestamp] CLIENT_XXX:SEQ_XXXXX` 形式か
3. **データ破損の有無**: 部分的な行や混在したデータがないか
4. **真の並列性**: ネイティブスレッドによる同時アクセス

## 期待される結果

```
==============================================
  Host Trait Concurrent Append Test
==============================================

Configuration:
  Clients:         3
  Appends/client:  50
  Expected lines:  150

Starting 3 threads with shared VFS...
[Client 1] Completed: 50 success, 0 errors
[Client 2] Completed: 50 success, 0 errors
[Client 3] Completed: 50 success, 0 errors

--- Verification ---
Total lines:   150
Valid lines:   150
Invalid lines: 0

--- First 20 lines ---
[1768657750415] CLIENT_001:SEQ_00000
[1768657750529] CLIENT_002:SEQ_00000
[1768657750542] CLIENT_003:SEQ_00000
...

==============================================
  TEST PASSED
==============================================

True concurrent access with proper locking: CONFIRMED
```

## RPC 版との比較

| 項目 | RPC 版 | Host Trait 版 |
|------|--------|---------------|
| 並列性 | 疑似並列 (WASM シングルスレッド) | 真の並列 (ネイティブマルチスレッド) |
| ロック検証 | ❌ TCP で自然に直列化 | ✅ fs-core のロックを検証 |
| 通信 | TCP:9000 経由 | 直接メモリ共有 |
| fs-core モード | Rc<RefCell> | Arc<RwLock> + DashMap |

## ファイル構成

- `append-client/` - 追記を行う WASM アプリ (RPC 版と共通)
- `host-runner/` - ネイティブホストプログラム (マルチスレッド)
- `run-test.sh` - テスト実行スクリプト
