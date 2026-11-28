# dcc-mcp-core

Conecte um agente de IA ou um pipeline de produção aos aplicativos de criação que sua equipe já usa. Com o dcc-mcp-core, um cliente pode procurar uma ferramenta no Maya, no Blender ou no Photoshop e executá-la na sessão em que a cena ou o documento está aberto. O núcleo, escrito em Rust e Python, oferece interfaces MCP (Model Context Protocol) e REST.

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core?label=PyPI)](https://pypi.org/project/dcc-mcp-core/) [![CI](https://img.shields.io/github/actions/workflow/status/dcc-mcp/dcc-mcp-core/ci.yml?branch=main&label=CI)](https://github.com/dcc-mcp/dcc-mcp-core/actions/workflows/ci.yml) [![MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

[English](README.md) · [简体中文](README_zh.md) · [日本語](README_ja.md) · [한국어](README_ko.md) · [Español](README_es.md) · [Français](README_fr.md) · [Deutsch](README_de.md) · **Português (Brasil)** · [Русский](README_ru.md) · [Türkçe](README_tr.md)

## Encontrar uma ferramenta em um aplicativo aberto

Instale a [ferramenta de linha de comando `dccmcp`](https://github.com/dcc-mcp/dcc-mcp-cli) e ative o [adaptador DCC-MCP](docs/adapter-coverage.md) do aplicativo. Se estiver trabalhando com um agente, a [Skill `dcc-mcp`](https://clawhub.ai/loonghao/skills/dcc-mcp) orienta o uso da mesma CLI. Para encontrar uma ferramenta que liste os objetos de uma cena do Maya, por exemplo, execute estas duas consultas.

~~~bash
dccmcp list
dccmcp search --query "list objects" --dcc-type maya
~~~

O comando `list` mostra as sessões registradas no gateway. Quando a busca encontra uma ferramenta, o campo `tool_slug` traz seu identificador. Consulte `next_step` para saber se é preciso carregar a Skill, conferir os argumentos aceitos ou chamar a ferramenta diretamente. A busca não altera a cena.

Se o Maya não aparecer na lista, confira se o adaptador está em execução e se a CLI está usando o gateway correto. Use `dccmcp doctor` para verificar a conexão. Quando uma solicitação falhar, anote o `request_id` para encontrá-la nos logs. A [referência da CLI](docs/guide/cli-reference.md) explica como configurar perfis de conexão, e o [guia de implantação remota](docs/guide/remote-server.md) cobre o uso em outra estação de trabalho.

## Executar um servidor de Skills com Python

Você pode testar o servidor sem abrir um aplicativo de criação. Clone o repositório, entre no diretório raiz e instale o pacote Python em um ambiente virtual.

~~~bash
python -m pip install dcc-mcp-core
~~~

Execute o código abaixo como um script Python. Ele disponibiliza o [exemplo de camadas do Photoshop](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers) em uma porta local livre. Conecte o cliente MCP à URL exibida no terminal. Para encerrar o servidor, pressione Ctrl+C nesse mesmo terminal.

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

Depois de conectar o cliente, procure `commercial-2d-layers` com `search_skills`. Se o pacote aparecer nos resultados, o servidor conseguiu encontrá-lo.

O exemplo mostra como organizar os arquivos de uma ferramenta. O [`SKILL.md`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/SKILL.md) identifica o aplicativo e aponta para o [`tools.yaml`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/tools.yaml). Nesse arquivo, `summarize_layers` é declarada como ferramenta somente de leitura, com um argumento opcional chamado `document`. O [script Python](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/scripts/summarize_layers.py) devolve um resumo predefinido. Para ler um documento aberto, implemente essa operação no adaptador do aplicativo. Confira a definição do pacote com o comando abaixo.

~~~bash
dccmcp lint examples/rez-skills/commercial_2d_layers/skills
~~~

O [guia de Skills](docs/guide/skills.md) explica como definir e validar ferramentas próprias. Para reaproveitar pacotes, consulte o [Marketplace](https://dcc-mcp.github.io/marketplace). Se o estúdio distribui ferramentas com Rez, o [guia de Rez](docs/guide/rez-skill-packages.md) mostra como iniciar o servidor em um ambiente com as dependências já resolvidas.

## Como uma chamada chega à sessão correta

![Caminho da chamada do cliente até o aplicativo, passando pelo gateway](docs/assets/architecture/current-stack.svg)

O cliente envia a solicitação ao gateway. O gateway localiza a sessão no registro e encaminha a chamada ao adaptador correspondente. O supervisor acompanha o estado do adaptador para saber se ele está pronto. O adaptador executa a ferramenta dentro do aplicativo e direciona o trabalho à thread principal quando a API do aplicativo exige.

- **Escolher uma sessão.** Se houver duas instâncias do mesmo aplicativo abertas, consulte `gateway://instances`. Confira a `mcp_url` e o estado de cada instância antes de escolher o destino. Veja o [guia do gateway](docs/guide/gateway.md).
- **Implementar um cliente.** Via HTTP, procure a ferramenta com `POST /v1/search`. Quando ela estiver pronta, envie `tool_slug` e `arguments` para `POST /v1/call`. Clientes MCP também podem usar Streamable HTTP e a extensão Agent Skills. Veja a [API REST](docs/guide/rest-api-surface.md) e o [guia de MCP Skills](docs/guide/mcp-skills-integration.md).
- **Repetir uma tarefa.** Agrupe as etapas em um fluxo de trabalho e configure o armazenamento persistente para retomar uma execução interrompida. Se uma etapa só estiver disponível na interface gráfica, o UI Control limita a entrada à janela escolhida. Veja os guias de [fluxos de trabalho](docs/guide/workflows.md) e [UI Control](docs/guide/ui-control-workflows.md).

## Aplicativos compatíveis

O catálogo inclui Maya, Blender, Houdini, Photoshop, Unreal, Unity, Godot e RenderDoc, entre outros. Execute `dccmcp dcc-types` para consultar os adaptadores presentes no catálogo da sua versão. A [lista completa de aplicativos](docs/adapter-coverage.md) identifica o adaptador de cada um e reúne os dados de instalação disponíveis.

Para conferir o número de entradas na sua cópia do repositório, execute este comando no diretório raiz.

~~~bash
rg -c '^    tags: \[.*"adapter".*\]' dcc-mcp-catalog.yml
~~~

O [catálogo](dcc-mcp-catalog.yml) desta versão tem **49 entradas de adaptadores**. Se quiser conectar um aplicativo desenvolvido pelo próprio estúdio, siga o [guia de criação de adaptadores](docs/guide/new-adapter-onboarding.md).

## Como contribuir

No diretório raiz do repositório, use as versões das ferramentas definidas pelo projeto para compilar o pacote de desenvolvimento e executar os testes de Python e Rust.

~~~bash
vx just install
vx just dev
vx just test
vx just test-rust
~~~

Depois de alterar a documentação, execute `vx just docs-check`. Quando mudar um comando ou um campo de resposta, atualize o exemplo em cada README. Escreva as explicações de um jeito natural em cada idioma e mantenha os exemplos executáveis em sincronia. Consulte o [guia de contribuição](CONTRIBUTING.md) e o [índice da documentação](docs/guide/INDEX.md).

O dcc-mcp-core é distribuído sob a [licença MIT](LICENSE). [Site do projeto](https://dcc-mcp.github.io/) · [Galeria de projetos](https://dcc-mcp.github.io/showcase) · [Versões](https://github.com/dcc-mcp/dcc-mcp-core/releases) · [Registro MCP](https://registry.modelcontextprotocol.io/?q=io.github.dcc-mcp%2Fdcc-mcp-core)
