# RPC Concurrent Append Test

複数の WASM クライアントが VFS RPC Server を介して同一ファイルに同時追記を行い、
適切なロック制御によりデータ競合が発生しないことを検証するテストです。

## 概要

```
┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
│  Client 1   │  │  Client 2   │  │  Client 3   │  │  Client 4   │
│ (append-    │  │ (append-    │  │ (append-    │  │ (append-    │
│  client)    │  │  client)    │  │  client)    │  │  client)    │
└──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘
       │                │                │                │
       │    TCP:9000    │                │                │
       └────────┬───────┴────────┬───────┴────────┬───────┘
                │                │                │
                ▼                ▼                ▼
         ┌─────────────────────────────────────────┐
         │           VFS RPC Server                │
         │  ┌─────────────────────────────────┐   │
         │  │   /shared/concurrent.log        │   │
         │  │   (fs-core with proper locking) │   │
         │  └─────────────────────────────────┘   │
         └─────────────────────────────────────────┘
```

## 実行方法

```bash
# デフォルト (4 クライアント × 100 追記 = 400 行)
./run-test.sh

# カスタム (8 クライアント × 500 追記 = 4000 行)
./run-test.sh 8 500

# Makefile から
make run-usecase-rpc-concurrent
```

## 検証内容

1. **行数の整合性**: 期待される行数 (クライアント数 × 追記回数) と一致するか
2. **フォーマットの正当性**: 各行が `CLIENT_XXX:SEQ_XXXXX` 形式か
3. **データ破損の有無**: 部分的な行や混在したデータがないか

## 期待される結果

```
=== Verification Result ===
PASS: All 400 lines verified, no data corruption

Concurrent append with proper locking: CONFIRMED
```

## ファイル構成

- `append-client/` - 追記を行う WASM アプリ
- `verify-result/` - 結果を検証する WASM アプリ
- `run-test.sh` - テスト実行スクリプト
