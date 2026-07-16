インストーラは CI ビルドの**未署名**バイナリです。初回起動時に OS の警告が出ます。

## Windows（`Alpha Radar_*_x64-setup.exe`）

SmartScreen の「WindowsによってPCが保護されました」→「詳細情報」→「実行」。

## macOS（`Alpha Radar_*_aarch64.dmg` — **Apple Silicon 専用**）

M1 以降の Mac 専用です（Intel Mac では起動しません）。署名・公証を行っていないため、**次の手順が必須**です。

1. dmg を開き、`Alpha Radar.app` を `アプリケーション` へドラッグ。
2. **ターミナルで以下を実行**（これを省略するとアプリは起動してもスキャンが必ず失敗します）:

   ```sh
   xattr -dr com.apple.quarantine "/Applications/Alpha Radar.app"
   ```

> **なぜ必要か**: 「開発元を確認できません」の警告を「このまま開く」で回避してもアプリ本体は起動しますが、**同梱のデータ取得プロセス（`fetch`）には個別に隔離属性が付いたまま**です。スキャン時に Gatekeeper がこれを停止するため、`sidecar error` でスキャンが失敗します。上記コマンドは `.app` 内のすべてに対して隔離属性を解除します。

## 動作確認

起動後、ティッカー（例: `7974` / `AAPL`）を入力して「スキャン実行」。結果が出れば同梱サイドカーが正常に動作しています。
