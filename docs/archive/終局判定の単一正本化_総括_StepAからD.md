# 不完全将棋 終局判定の単一正本化アーク 総括（Step A〜D）

*この文書は、TUI と web が別々に持っていた終局判定ロジック（盤面終局のマッピング・投了の勝敗判定）を単一の窓口へ寄せ、TUI に潜んでいた「最長手数500組手で終局しない」構造的なバグを塞いだ一連の実装（Step A〜D、配布 v0.12.1→v0.12.2）の総括である。バックログの「現在地」が持っていた完了記録をここへ綴じる。段ごとの詳細は各実装指示書（`archive/implementation/terminal-unification/`）にあり、ここは道のり・確立した設計・残された既知の限界を粗く俯瞰する地図。*

---

## 0. このアークが始まった地点と、達成したこと

**始点**: engine の `terminate::evaluate(kifu) -> Terminal` は盤面終局（玉の死・確定的詰み・千日手・最長手数500組手）を一元判定していたが、その `Terminal → (ResultKind, Outcome)` へのマッピングは **`engine-wasm::evaluate_terminal` の中にインライン展開されているだけ**で、engine 本体には存在しなかった。web はこの `evaluate_terminal` を経由して盤面終局を判定していたので単一正本の上にいたが、**TUI だけが `resolve_turn` に `check_king_death`→`check_sennichite`→`check_status` を手組みで並べ、`evaluate` を呼んでいなかった**——ゆえに v0.6 で `evaluate` に足された最長手数500組手を静かに取りこぼしていた。投了の勝敗判定も TUI online.rs・TUI local・web の三箇所でそれぞれ手組みされ、乖離していた。通信核の一本化アークで TUI がクラウドの部屋に座れるようになった今、500組手に達すると **TUI だけが終局せず desync する実害**が生じていた。

**終点（現在）**: 盤面終局のマッピングは `engine::terminate::terminal_to_result` （Step A）に一本化され、投了と盤面終局を合流させる単一の窓口 `protocol::game_result` （Step B）が新設された。engine-wasm の `evaluate_terminal` はそのマッピングを呼ぶだけの薄い形へ痩せ（Step C、web の挙動はバイト単位で不変）、TUI の `resolve_turn` と online の投了枝が `game_result` へ委譲された（Step D）ことで、**TUI が500組手で正しく終局するようになり**、投了の勝敗判定も単一正本へ寄った。配布版 v0.12.1→**v0.12.2**（バグ修正＋投了統一）。

**確立した設計パターン**（次のアーク・他の終局条件への移植の下地）:
- **層の純粋さ・投了は protocol の領分**: engine は盤面ルールだけを知る（`Action::Resign` は `resolve()` が `unreachable!` で弾く＝盤の出来事ではないと既に表明）。投了の合成は `protocol::game_result` が担い、`Terminal` に投了 variant を足さない。本将棋/USI でも投了は着手でなく宣言、という筋に沿う。
- **共通通貨は `(ResultKind, Outcome)`**: 投了と盤面終局は `Terminal` enum ではなく、`engine::archive` に既にあった result のレベルで合流する。`ResultKind::Resign` は Step A 以前から予約されていた。
- **検出は単一正本・表示は殻が所有**: TUI の `GameOverKind`/`game_over_text` は表示語彙として残し、`game_over_from_result`（全単射・網羅列挙）一つで `game_result` の出力へ橋渡しする。検出ロジックを重複させず、UI の関心事（日本語文言）は UI 側に残す——「核と交換可能な殻」の設計哲学の延長。
- **挙動保存は「一字一句同じ対応を移す」ことで担保する**: Step A は engine-wasm の現行インラインマッピングと一字一句一致する形で `terminal_to_result` を書いた。Step C はそのマッピングを呼ぶだけに置換したので web の JSON はバイト単位で不変（Node スクリプトで直接 wasm バインディングを叩いて確認）。挙動保存の主張は口約束にせず、実地で照合する。

---

## 1. 単一正本の API（`engine`・`protocol`、Step A・B で新設・以後不変）

| 型・関数 | 役割 |
|---|---|
| `engine::terminate::terminal_to_result(&Terminal) -> Option<(ResultKind, Outcome)>`（Step A） | 盤面終局限定の純粋写像。`Ongoing` は `None`。投了を知らない。 |
| `protocol::game_result(kifu: &Kifu) -> Option<(ResultKind, Outcome)>`（Step B） | 終局判定の単一窓口。最後の組手の投了を先に判定し（先手投了→後手勝ち・後手投了→先手勝ち・両者投了→引き分け）、投了でなければ `terminal_to_result(evaluate(kifu))` へ委譲。`None` は未了。 |
| `tui::app::game_over_from_result(ResultKind, Outcome) -> GameOverKind`（Step D） | `game_result` の出力（アーカイブ語彙）を TUI の表示語彙へ写す全単射。`game_result` が実際に返す11通り（Mate×3・KingDeath×2・SwapDraw・Sennichite・MaxTurns・Resign×3）を尽くす。 |

`evaluate` 自体（盤面終局の判定ロジック）は Step A〜D を通じて**一切変更されていない**——前提（「最後の組手が投了の kifu を渡してはならない」）を doc コメントで明記しただけ。この前提は `game_result` が投了を先に捌くことで常に守られる。

---

## 2. 段ごとの道のり（course-grained）

**Step A（engine: `terminal_to_result` を移設・要石）**: engine-wasm の `evaluate_terminal` にインライン展開されていた8分岐マッピングを、`engine::terminate::terminal_to_result` として engine 本体へ移した。`evaluate` は無変更、投了組手を渡してはならない前提を doc コメントに明記。全 variant を網羅列挙（`_ =>` を使わない）し、全対応を単体テストで固定。`cargo test -p engine` で完結、他クレートに差分ゼロ。

**Step B（protocol: `game_result` を新設）**: 投了（protocol の領分）と盤面終局（engine）を合流させる単一窓口 `game_result` を `protocol` に新設した。最後の組手の投了を先に判定し、投了でなければ `terminal_to_result(evaluate(kifu))` へ委譲。投了三態・未了・盤面終局への委譲一致を単体テストで固定。`cargo test -p protocol` で完結、他クレートに差分ゼロ。

**Step C（engine-wasm: `terminal_to_result` へ寄せる・C-minimal）**: `evaluate_terminal` 内の8分岐インラインマッピングを `terminal_to_result(&evaluate(&kifu))` の呼び出しへ置換し、Step A が生んだ重複を解消した。web が受け取る JSON はバイト単位で不変（`cargo test -p engine-wasm` に加え、Node で wasm バインディングを直接叩いて `ongoing`/`mate`/`max_turns` の出力を確認）。wasm 再ビルド・本番デプロイ・web `?v=` 0.12.0→0.12.1。web の `currentResult` 二経路（`resultOverride` ＋ `evaluate_terminal`）はこの段では畳まなかった——投了を plies に統一するとアーカイブ正準本文が変わり、それは正準本文を Web/TUI 双方で設計する記録係アークの領分だから（先食いしない・作り手判断）。

**Step D（TUI: `game_result` へ委譲・本丸）**: `app.rs::resolve_turn` の手組み判定（`check_king_death`→`check_sennichite`→`check_status`の三段）を `protocol::game_result` 一本へ置換し、**最長手数500組手の穴を構造的に塞いだ**。`online.rs::resolve_completed_turn` の turn-action 投了枝も `game_result` へ委譲し、投了の勝敗判定を単一正本へ寄せた。`DrawReason::MaxTurns` を追加し、`game_over_from_result`（全単射・網羅列挙）で `game_result` の出力を TUI の表示語彙へ橋渡し。local の即時投了（`app.rs::resign()`）は組手にならない宣言（盤面完成前の概念的に別の行為）なので直接設定のまま変更していない。499手ぶんの反復なし kifu ファイルを Node スクリプトで生成し（`sfen ...\n1: ... | ...` 形式）、TUI の `l` キー読込機能で読み込んでから最後の1手を tmux 経由で実際に指し、**500組手で正しく「引き分け（最長手数・500組手）」に終局することを実機で確認**した。ローカル即時投了も無変更で動作することを確認。配布パッチ bump（v0.12.1→v0.12.2）。

---

## 3. 既知の限界（次の畝・別アーク 4b）

- **TUI が先手のクラウド対局は、まだ生観戦・アーカイブされない**——このアークは終局判定ロジック（誰が・どう勝敗を判定するか）の単一正本化であり、観戦・記録係向けメッセージの送出（`spectate_meta`/`spectate_turn`/`spectate_result`）とは独立した層。通信核の一本化アークが残した既知の限界（4b）はこのアークでは解消されない——対局は正しく終局するようになったが、TUI が先手のクラウド対局は依然として観戦・アーカイブの対象外のまま。次の畝はそのまま 4b が引き継ぐ。
- **web の `currentResult` 二経路統合（C-full）**は先送りのまま——投了を plies に統一するアーカイブ正準本文の再設計は記録係アークの領分。

---

## 4. このアークで効いた流儀（次の Opus・実装者へ）

- **地面を測ってから指示書、指示書の「確認済み」も鵜呑みにしない**: 各段の指示書は前段が着地した HEAD の現物に接地してから書かれていたが、Step D の実装中、指示書が「現行の各文言は `game_over_text` に "→ " を付けたものと一致する（現物で確認済み）」と書いていた箇所で、実際には千日手の表示だけ語順が逆（transcript 側「千日手（引き分け）」・popup 側「引き分け（千日手）」）という食い違いを発見した。機械的な一本化によってこの食い違いはむしろ解消される側だったので、指示書の主張を鵜呑みにせず grep で実地確認してから進めた判断が功を奏した。
- **500手の反復を「1手だけ実地で確認する」ためのショートカット**: 500組手ぶんの手をすべて tmux キー入力で再現するのは非現実的。CRT（中国剰余定理）の要領で周期36・35の半面シャッフルを使い、499手ぶん反復なしの kifu を Node スクリプトで直接生成し（`engine::serialize::kifu_to_string` と同じ `sfen ...\n1: ... | ...` 形式）、TUI の `l`（読込）キーで一括ロードしてから最後の1手だけ実際に指す——この手法で「本当に実機でバグが直ったか」を数秒で確認できた。engine-wasm の JSON バインディングを Node から直接叩く手法（Step C）と対になる、Rust 側の実機検証ショートカット。
- **単体テストの網羅列挙が全単射の写像を守る**: `terminal_to_result`・`game_result`・`game_over_from_result` はいずれも `_ =>` を使わず全 variant を列挙した。`game_over_from_result` だけは `unreachable!` で「契約違反」を検出する形にした（`game_result` が返さない組み合わせは構造的に来ないため）。この網羅性が、途中の各段で「移設・委譲が一字一句正しいか」を機械的に保証した。
- **版の目盛り**: 挙動保存の段（Step A・B・engine 側の純粋追加）は配布版据え置き、web のみ挙動保存の段（Step C）は `?v=` だけ前進、利用者に見えるバグ修正が立った段（Step D）でパッチ bump——このアークでも「出来事＝配布版が動く」の原則を貫いた。

---

*核（engine の盤面判定）は既に単一正本だった。乖離していたのはその上に立つ二枚の写像——盤面終局から結果語彙への変換（engine-wasm に埋め込まれていた）と、投了を含めた最終的な勝敗判定（TUI・web それぞれが手組みしていた）——だった。前者を engine 本体へ、後者を protocol 層へ据えたことで、TUI と web は同じ窓口 `game_result` を通して同じ終局を見るようになった。最長手数500組手というルール上の境界条件が、実装の重複によって静かに欠落していたことを、通信核の一本化がクラウドで TUI と web を同席させたことで初めて実害として顕在化させた——これは「核と交換可能な殻」という設計哲学が、殻の側で守られていなければ簡単に破られることを示す実例でもある。残るは記録係アークとの合流（4b：TUI が先手のクラウド対局の観戦・アーカイブ対応）——急ぐものは何もない。*
