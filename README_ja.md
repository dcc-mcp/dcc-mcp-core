# dcc-mcp-core

スタジオで使っている制作アプリを、AI エージェントや制作パイプラインから操作できます。dcc-mcp-core を使うと、クライアントは Maya、Blender、Photoshop などのツールを検索し、対象のシーンやドキュメントを開いているセッションで実行できます。Rust と Python で実装した共通ランタイムが、MCP（Model Context Protocol）と REST の両方に対応しています。

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core?label=PyPI)](https://pypi.org/project/dcc-mcp-core/) [![CI](https://img.shields.io/github/actions/workflow/status/dcc-mcp/dcc-mcp-core/ci.yml?branch=main&label=CI)](https://github.com/dcc-mcp/dcc-mcp-core/actions/workflows/ci.yml) [![MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

[English](README.md) · [简体中文](README_zh.md) · **日本語** · [한국어](README_ko.md) · [Español](README_es.md) · [Français](README_fr.md) · [Deutsch](README_de.md) · [Português (Brasil)](README_pt-BR.md) · [Русский](README_ru.md) · [Türkçe](README_tr.md)

## Skills over MCP

**Skills over MCP** と MCP `2026-07-28` に対応したクライアントは、拡張を公開する DCC エンドポイントで Agent Skills を検出し、必要に応じて `SKILL.md` と関連ファイルを取得できます。既存のクライアントとスクリプトは、引き続きゲートウェイの REST API 経由で `dccmcp` を使えます。[統合ガイド](docs/guide/mcp-skills-integration.md)

## 起動中のアプリでツールを探す

[`dccmcp` CLI](https://github.com/dcc-mcp/dcc-mcp-cli) をインストールし、使いたいアプリで対応する [DCC-MCP アダプター](docs/adapter-coverage.md)を有効にします。エージェントから操作する場合は、[`dcc-mcp` Skill](https://clawhub.ai/loonghao/skills/dcc-mcp) に CLI の使い方が用意されています。たとえば、Maya でシーン内のオブジェクトを一覧にするツールを探すには、次の 2 つのコマンドを実行します。

~~~bash
dccmcp list
dccmcp search --query "list objects" --dcc-type maya
~~~

`list` で、ゲートウェイに登録されているセッションを確認できます。`search` でツールが見つかると、その識別子が `tool_slug` に入ります。続いて `next_step` を確認してください。必要な Skill を読み込むのか、引数を確認するのか、そのまま呼び出せるのかが分かります。検索だけでシーンが変更されることはありません。

一覧に Maya が出てこない場合は、アダプターが起動しているか、CLI の接続先が意図したゲートウェイかを確認します。接続状態は `dccmcp doctor` で調べられます。リクエストに失敗したら、ゲートウェイのログをたどれるように `request_id` を控えておいてください。接続先の設定は [CLI リファレンス](docs/guide/cli-reference.md)、別のワークステーションへの導入は[リモートサーバーガイド](docs/guide/remote-server.md)にまとめています。

## Python で Skill サーバーを試す

サーバーの動作は、制作アプリを起動せずに確認できます。リポジトリをクローンし、ルートディレクトリで Python の仮想環境を有効にしてパッケージをインストールします。

~~~bash
python -m pip install dcc-mcp-core
~~~

次のコードを Python スクリプトとして実行すると、ローカルの空きポートで [Photoshop レイヤーのサンプル](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers)が利用できます。表示された URL に MCP クライアントを接続してください。サーバーを終了するには、スクリプトを実行した端末で Ctrl+C を押します。

~~~python
import time

from dcc_mcp_core import McpHttpConfig, create_skill_server

config = McpHttpConfig(port=0)
config.gateway_port = 0
server = create_skill_server(
    "photoshop",
    config,
    extra_paths=["examples/rez-skills/commercial_2d_layers/skills"],
    accumulated=False,
)
handle = server.start()
try:
    print(handle.mcp_url(), flush=True)
    while True:
        time.sleep(1)
except KeyboardInterrupt:
    pass
finally:
    handle.shutdown()
~~~

表示された DCC エンドポイントで `server/discover` を呼び出し、`io.modelcontextprotocol/skills` が公開されていることを確認します。次に `skills/list` で `commercial-2d-layers` を探します。従来の MCP クライアントでは `search_skills` で同じパッケージを探せます。

このサンプルでは、ツールを構成するファイルを順に確認できます。[`SKILL.md`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/SKILL.md) で対象アプリとツール定義の場所を指定し、[`tools.yaml`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/tools.yaml) で `summarize_layers` を読み取り専用として宣言しています。引数 `document` は省略できます。[Python スクリプト](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/scripts/summarize_layers.py)が返すのは固定のサンプルデータです。実際に開いているドキュメントを扱う処理は、アプリのアダプター側に実装します。パッケージの定義は次のコマンドで検証できます。

~~~bash
dccmcp lint examples/rez-skills/commercial_2d_layers/skills
~~~

ツールを自作する場合は [Skills ガイド](docs/guide/skills.md)、既存のパッケージを探す場合は [Marketplace](https://dcc-mcp.github.io/marketplace) を参照してください。Rez を使って配布する場合は、[Rez ガイド](docs/guide/rez-skill-packages.md)で解決済みのパッケージ環境からサーバーを起動する手順を確認できます。

## 呼び出しが対象のセッションに届くまで

![クライアントからゲートウェイを経由して制作アプリに届くまでの経路](docs/assets/architecture/current-stack.svg)

クライアントのリクエストを受け取ると、ゲートウェイは登録情報から対象のセッションを特定し、対応するアダプターに呼び出しを転送します。アダプターが実行可能な状態かどうかはスーパーバイザーが監視します。ツールはアプリ内で動作し、ホスト API が要求する場合はアダプターが処理をメインスレッドに渡します。

- **セッションを選ぶ。** 同じアプリを 2 つ起動している場合は、`gateway://instances` で各セッションの `mcp_url` と呼び出しを受け付けられる状態かどうかを確認します。詳しくは[ゲートウェイガイド](docs/guide/gateway.md)を参照してください。
- **クライアントを実装する。** HTTP では `POST /v1/search` でツールを検索し、呼び出せる状態になったら `tool_slug` と `arguments` を `POST /v1/call` に送ります。MCP では Streamable HTTP と Agent Skills 拡張を利用できます。[REST API](docs/guide/rest-api-surface.md) と [MCP Skills ガイド](docs/guide/mcp-skills-integration.md)に詳細があります。
- **手順を繰り返す。** 複数の操作をワークフローにまとめ、永続ストレージを設定すれば中断した処理を再開できます。画面でしか行えない操作には、入力先を特定のウィンドウに限定する UI Control を使えます。[ワークフロー](docs/guide/workflows.md)と [UI Control](docs/guide/ui-control-workflows.md) を参照してください。

## 対応アプリ

Maya、Blender、Houdini、Photoshop、Unreal、Unity、Godot、RenderDoc などに対応しています。リリースのカタログに含まれるアダプターは `dccmcp dcc-types` で確認できます。[対応アプリ一覧](docs/adapter-coverage.md)には、各アプリのアダプターと、用意されているインストール情報が載っています。

手元のリポジトリで登録件数を確かめるには、ルートディレクトリで次のコマンドを実行します。

~~~bash
rg -c '^    tags: \[.*"adapter".*\]' dcc-mcp-catalog.yml
~~~

この版の[カタログ](dcc-mcp-catalog.yml)には **49 件のアダプター**が登録されています。スタジオ独自のアプリを接続する手順は、[アダプター開発ガイド](docs/guide/new-adapter-onboarding.md)にまとめています。

## 開発に参加する

リポジトリのルートで次のコマンドを実行すると、プロジェクト指定のツールチェーンで開発用パッケージをビルドし、Python と Rust のテストを実行できます。

~~~bash
vx just install
vx just dev
vx just test
vx just test-rust
~~~

ドキュメントを変更したら `vx just docs-check` を実行してください。コマンドやレスポンスのフィールドを変更した場合は、各言語の README の例も更新します。説明はそれぞれの言語で読みやすく書き、実行例の内容をそろえてください。[コントリビューションガイド](CONTRIBUTING.md)と[ドキュメント一覧](docs/guide/INDEX.md)も参照できます。

dcc-mcp-core は [MIT ライセンス](LICENSE)で公開しています。[プロジェクトサイト](https://dcc-mcp.github.io/) · [活用事例](https://dcc-mcp.github.io/showcase) · [リリース](https://github.com/dcc-mcp/dcc-mcp-core/releases) · [MCP Registry](https://registry.modelcontextprotocol.io/?q=io.github.dcc-mcp%2Fdcc-mcp-core)
