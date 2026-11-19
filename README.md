# What it does

このアプリケーション（**ur-scanner**）は、UR都市機構（UR賃貸住宅）の対象団地の空き部屋情報を定期的に自動監視し、新しい空室が見つかった際に通知を送るバックエンドサービスです。

主な機能と仕組みは以下の通りです。

* **定期的な空室スキャン:** 指定した団地リストを読み込み、URの公式サイトから最新の募集住戸情報を定期的に取得します（スキャン間隔は10分おき）。
* **新規募集の検知と状態管理:** 取得した空室一覧と、すでに通知済みの部屋データ（Supabaseに保存・メモリ上の `HashSet` で管理）を比較します。新しく出た空室があれば通知を送り、募集が終了した部屋は履歴から自動的に削除します（将来再び空きが出た際に再通知できるようにするため）。
* **HTTP APIサーバー（Axum）:** スキャナー本体と並行してWeb APIサーバーを起動し、ヘルスチェックや外部からのリクエストを受け付けます。
* **可観測性（OpenTelemetry）:** 各スキャンの所要時間、成功・エラー回数、最終成功時刻などのメトリクスや分散トレースを収集し、システムの状態を監視できるようにしています。
* **安全なシャットダウン（Graceful Shutdown）:** Kubernetes（Helm）でのデプロイ更新や `Ctrl+C` による停止シグナルを受け取った際、待機中であれば即座に終了し、スキャン実行中であればデータの不整合を防ぐため現在の処理が完了するまで待ってから安全に終了します。

# How to deploy

```bash
podman build -t ur-scanner:latest .
podman run --rm ur-scanner:latest
podman tag ur-scanner:latest xyz/ur-scanner:latest
podman push xyz/ur-scanner:latest
```

Helm chart:

```bash
helm upgrade --install ur-scanner .   -n ur-scanner   --create-namespace   -f values.yaml
```
