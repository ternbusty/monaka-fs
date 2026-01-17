# Lock Storategy Benchmark

```
cd usecases/benchmarks/06-lock-strategy/bench-runtime

./run.sh build      # Build
./run.sh test       # 簡易正当性テスト
./run.sh fine       # lock-fine ベンチマーク 
./run.sh global     # lock-global ベンチマーク     
./run.sh unsafe     # lock-none ベンチマーク（クラッシュの可能性あり）  
./run.sh all        # 全ベンチマーク実行 
```