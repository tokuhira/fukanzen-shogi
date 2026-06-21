# 不完全将棋 実装指示書 — GitHub Actions による Windows ビルド（最小構成・学習の第一歩）

> 対象実行者: Claude Code
> 前提: ワークスペースは `engine`・`cli`・`tui`（および Phase3 の通信関連クレート）からなる Rust プロジェクト。TUI は既にネットワーク対戦に対応し、コマンドライン引数で対戦モードに入れる。
> 関連文書: リポジトリの `tui/Cargo.toml`（パッケージ名・バイナリ名の出典）。
> 性格: GitHub Actions を初めて導入する**最小構成**。Intel Windows（x86_64-pc-windows-msvc）向けに TUI バイナリだけをビルドし、Artifacts にアップロードする。リリース作成・複数ターゲット・タグ発火は**やらない**（後の段階で足す）。学びを段階的に進めるための第一歩。

---

## 0. 目的と範囲（最小構成）

- **作るもの**: `.github/workflows/` に、push をトリガーに、Windows ランナー上で TUI バイナリを `x86_64-pc-windows-msvc` でビルドし、生成された `.exe` を Artifacts にアップロードするワークフロー一つ。
- **作らないもの（このステップの非ゴール）**: GitHub Releases へのリリース添付、複数ターゲット（Linux/macOS/ARM）のマトリクス、タグ発火、CLI など TUI 以外のバイナリ、クロスコンパイル（Linux ランナーからの Windows ビルド等）。これらは後続ステップで段階的に足す。
- **検証の出口**: ワークフローが緑になり、Artifacts から `.exe` をダウンロードできること。ダウンロードした Intel 64bit バイナリを、利用者の ARM Windows 上で x64 エミュレーションにより起動して TUI が表示できること（ビルドの正しさの最終確認）。

> このステップの真の目的は「動く .exe を得る」ことと同じくらい、**GitHub Actions の挙動（トリガー・ランナー・ステップ・ログ・Artifacts）を体感して学ぶ**ことにある。一発で緑にすることより、失敗ログを読んで直すサイクルに慣れることを重視してよい。

---

## 1. 事前確認（決め打ちにしない）

ワークフローを書く前に、リポジトリの実物から次を確認し、その値を使うこと（GitHub 上のキャッシュ表示ではなく、ローカルの実ファイルを見る）:

- **TUI のパッケージ名**: `tui/Cargo.toml` の `[package] name`。
- **TUI のバイナリ名**: `tui/Cargo.toml` に `[[bin]] name` があればその名前。無ければバイナリ名はパッケージ名と同じ。Windows では末尾に `.exe` が付く（例: パッケージ名が `fukanzen-shogi-tui` なら成果物は `fukanzen-shogi-tui.exe`）。
- **ワークスペース構成**: ルート `Cargo.toml` が `[workspace]` で、`tui` がそのメンバーであること。

> 以降、`<TUI_PKG>` を TUI のパッケージ名、`<TUI_BIN>` をバイナリ名（`.exe` を除いた名前）として記す。ワークフローでは実際の値に置き換える。

---

## 2. ワークフローの設計

### 2.1 配置

`.github/workflows/windows-build.yml`（ファイル名は任意。例として）。

### 2.2 トリガー

- まずは**無条件の push トリガー**で始める（`on: push`）。main への push のたびに走り、挙動を体感する。
- 加えて、手動実行できるよう `workflow_dispatch` も付けておく（Actions 画面から「Run workflow」で試せて、学習に便利）。
- 補足: 毎 push で走るのが煩わしくなったら、後で `paths`（`**.rs`、`Cargo.toml`、`Cargo.lock`、ワークフロー自身）で絞れる。**最初は絞らず**、素直に毎 push で回して挙動を見る。

### 2.3 ジョブとランナー

- ランナーは `windows-latest`（GitHub ホストの x64 Windows ランナー）。この上で `x86_64-pc-windows-msvc` を**ネイティブにビルド**する（クロスコンパイル不要、msvc ツールチェーンの追加設定不要）。

### 2.4 ステップ

おおむね次の順:

1. **チェックアウト**: `actions/checkout` でリポジトリを取得。
2. **Rust ツールチェーン**: Windows ランナーには Rust が同梱されているが、確実を期すため Rust の安定版セットアップを明示する（`rustup` は既に入っているので、必要なら `rustup default stable` 程度。target `x86_64-pc-windows-msvc` はランナーの既定ホストなので追加 install は通常不要）。
3. **（任意）依存キャッシュ**: ビルド時間短縮のため Cargo のキャッシュを使うステップを入れてよい（学習段階では省略しても可。入れるなら定番のキャッシュアクションを使う）。
4. **ビルド**: TUI バイナリだけをリリースビルドする。ワークスペース全体ではなく TUI を名指しする:
   ```
   cargo build --release -p <TUI_PKG> --bin <TUI_BIN> --target x86_64-pc-windows-msvc
   ```
   `--target` を明示すると成果物は `target/x86_64-pc-windows-msvc/release/<TUI_BIN>.exe` に出る（`--target` を付けない場合はランナー既定の `target/release/` に出る。**出力パスとアップロードのパスを一致させること**——ここがよくあるつまずき）。
5. **Artifacts アップロード**: `actions/upload-artifact` で `.exe` を上げる。`path` は前ステップの実際の出力パスに合わせる（例: `target/x86_64-pc-windows-msvc/release/<TUI_BIN>.exe`）。`name`（Artifact 名）は分かりやすく（例: `fukanzen-shogi-tui-windows-x64`）。

### 2.5 最小の YAML の骨子（値は §1 で確認した実名に置換）

```yaml
name: Windows Build

on:
  push:
  workflow_dispatch:

jobs:
  build-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - name: Set up Rust
        run: rustup default stable
      - name: Build TUI (release, msvc)
        run: cargo build --release -p <TUI_PKG> --bin <TUI_BIN> --target x86_64-pc-windows-msvc
      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: fukanzen-shogi-tui-windows-x64
          path: target/x86_64-pc-windows-msvc/release/<TUI_BIN>.exe
```

> アクションのメジャーバージョン（`@v4` 等）は、現時点で利用可能な安定版を使うこと。古いメジャーは非推奨・廃止されることがあるため、`checkout` と `upload-artifact` は最新の安定メジャーを確認して指定する。

---

## 3. 学びの観察ポイント・つまずき箇所

このステップは学習が主目的。次を意識する:

- **一発で緑にならないのが普通**。失敗（赤）したら、Actions のジョブログを開き、どのステップでどんなメッセージが出たかを読む。この「赤→ログ→修正」のサイクルが学びの本体。
- よくあるつまずき:
  - **パッケージ名／バイナリ名の綴り違い** → §1 で実ファイルから確認した値を使う。`-p` の指定が誤ると「package not found」になる。
  - **出力パスとアップロードパスの不一致** → `--target` を付けると `target/<target>/release/` に、付けないと `target/release/` に出る。`.exe` の付け忘れも頻出。アップロードの `path` をビルドの実出力に合わせる。
  - **YAML のインデント崩れ** → ステップやキーのインデントずれで構文エラー。
  - **アクションの古いメジャーバージョン** → `upload-artifact@v3` 等の旧版は廃止されていることがある。最新安定メジャーを使う。
- **Artifacts の確認**: 緑になったら、当該ワークフロー実行のページ下部「Artifacts」から `.exe` をダウンロードできる。これが成果物の受け取り口（リリースはまだ作らない）。

---

## 4. 動作確認（ビルドの正しさの最終確認）

1. Artifacts から `<TUI_BIN>.exe` をダウンロードする。
2. ARM Windows 上で起動する（Windows on ARM の x64 エミュレーションにより Intel 64bit バイナリが動く）。**新しい Windows Terminal で実行する**ことを推奨（TUI の表示・文字コード・キー入力の相性が良い）。
3. TUI が表示され、コマンドライン引数で対戦モードに入れること（既存の対戦モード起動引数）を確認する。表示が崩れる場合はフォント・コードページ（UTF-8）・ターミナルの種類を見直す。これはビルドではなく実行環境の問題なので、ビルド成果物自体の正しさとは切り分けて考える。

> Windows ターミナルでの TUI（ratatui/crossterm）の表示は、ビルドが正しくても実行環境（ターミナル種別・フォント・コードページ）に依存する。「ビルドは通る／Artifacts は得られる」と「実機で綺麗に表示される」を分けて確認すること。

---

## 5. 完了基準

1. `.github/workflows/` にワークフローが追加され、push（または手動実行）で起動する。
2. Windows ランナー上で TUI バイナリが `x86_64-pc-windows-msvc` でビルドされ、ワークフローが緑になる。
3. Artifacts から `<TUI_BIN>.exe` をダウンロードできる。
4. ダウンロードした `.exe` が ARM Windows 上（x64 エミュレーション）で起動し、TUI と対戦モード起動が確認できる。
5. `engine`・`cli`・`tui` のコードは無改変（ワークフロー追加のみ。ビルド設定の都合で `Cargo.toml` に手を入れる場合は最小限に留め、報告する）。

---

## 6. 次の段階（このステップでは作らない・見通しだけ）

この最小ワークフローが動いたら、学びを段階的に広げられる:

- **複数ターゲットのマトリクス化**: Windows に加え Linux（`x86_64-unknown-linux-gnu`）、macOS（`aarch64-apple-darwin` 等）を `strategy.matrix` で並列ビルド。
- **タグ発火 ＋ GitHub Releases へ添付**: `on: push: tags: ['v*']` で、バージョンタグを打つとビルド成果物をリリースに自動添付。タグ＝リリースの明快な対応。
- **paths による絞り込み**: Rust 関連ファイルが変わったときだけ走らせる。
- **キャッシュ最適化**: Cargo の依存・ビルドキャッシュで時間短縮。
- **開発環境向け（任意）**: WS(ARM Linux)用は普段のローカル `cargo build`（`aarch64-unknown-linux-gnu`）で足りるため、Actions で作る必然性は低い。必要になったらマトリクスに足す。

各拡張は、この最小ワークフローを土台に一項ずつ足す形で学べる。一度に完成形を目指さず、緑を確認しながら登る。

---

*GitHub Actions Windows ビルド 指示書 v1.0（最小構成）— Intel Windows（msvc）向け TUI バイナリを Windows ランナーでネイティブビルドし Artifacts に上げる、学習の第一歩。リリース・マトリクス・タグ発火は後続。コードは無改変、ワークフロー追加のみ。*
