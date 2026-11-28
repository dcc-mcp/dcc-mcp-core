# dcc-mcp-core

С помощью dcc-mcp-core можно подключить ИИ-агента или производственный пайплайн к приложениям, которыми уже пользуется студия. Клиент находит нужный инструмент в Maya, Blender или Photoshop и запускает его в том сеансе, где открыта нужная сцена или документ. Общее ядро написано на Rust и Python и поддерживает MCP (Model Context Protocol) и REST.

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core?label=PyPI)](https://pypi.org/project/dcc-mcp-core/) [![CI](https://img.shields.io/github/actions/workflow/status/dcc-mcp/dcc-mcp-core/ci.yml?branch=main&label=CI)](https://github.com/dcc-mcp/dcc-mcp-core/actions/workflows/ci.yml) [![MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

[English](README.md) · [简体中文](README_zh.md) · [日本語](README_ja.md) · [한국어](README_ko.md) · [Español](README_es.md) · [Français](README_fr.md) · [Deutsch](README_de.md) · [Português (Brasil)](README_pt-BR.md) · **Русский** · [Türkçe](README_tr.md)

## Как найти инструмент в открытом приложении

Установите [консольную утилиту `dccmcp`](https://github.com/dcc-mcp/dcc-mcp-cli) и включите в приложении соответствующий [адаптер DCC-MCP](docs/adapter-coverage.md). Если вы работаете через агента, [навык `dcc-mcp`](https://clawhub.ai/loonghao/skills/dcc-mcp) подскажет ему, как пользоваться той же утилитой. Например, чтобы найти инструмент для просмотра списка объектов в сцене Maya, выполните две команды.

~~~bash
dccmcp list
dccmcp search --query "list objects" --dcc-type maya
~~~

`list` показывает сеансы, зарегистрированные на шлюзе. Если `search` находит инструмент, в поле `tool_slug` будет его идентификатор. Поле `next_step` подсказывает, нужно ли загрузить пакет навыка (Skill), уточнить аргументы или можно сразу вызвать инструмент. Сам поиск не меняет сцену.

Если Maya нет в списке, проверьте, запущен ли её адаптер и к тому ли шлюзу подключена утилита. Команда `dccmcp doctor` помогает проверить соединение. Если запрос завершился ошибкой, сохраните `request_id`, чтобы найти его в журнале шлюза. Настройка профилей описана в [справочнике CLI](docs/guide/cli-reference.md), а подключение другой рабочей станции — в [руководстве по удалённому развёртыванию](docs/guide/remote-server.md).

## Как запустить сервер навыков на Python

Сервер можно опробовать без запуска графического приложения. Клонируйте репозиторий, перейдите в его корневой каталог и установите пакет Python в виртуальное окружение.

~~~bash
python -m pip install dcc-mcp-core
~~~

Запустите следующий код как скрипт Python. Сервер откроет свободный локальный порт и предоставит доступ к [примеру для слоёв Photoshop](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers). Подключите клиент MCP к адресу, который появится в терминале. Чтобы завершить работу сервера, нажмите Ctrl+C в том же терминале.

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

После подключения найдите `commercial-2d-layers` с помощью `search_skills`. Если пакет появился в результатах, сервер обнаружил пример.

По этому примеру можно разобраться, из чего состоит инструмент. Файл [`SKILL.md`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/SKILL.md) указывает целевое приложение и ссылается на [`tools.yaml`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/tools.yaml). В нём `summarize_layers` объявлен как инструмент только для чтения с необязательным аргументом `document`. [Скрипт Python](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/scripts/summarize_layers.py) возвращает заранее подготовленную сводку. Для чтения открытого документа потребуется реализовать обработчик в адаптере приложения. Проверить описание пакета можно командой ниже.

~~~bash
dccmcp lint examples/rez-skills/commercial_2d_layers/skills
~~~

О том, как описать и проверить собственные инструменты, читайте в [руководстве по навыкам](docs/guide/skills.md). Готовые пакеты можно найти в [Marketplace](https://dcc-mcp.github.io/marketplace). Если студия распространяет инструменты через Rez, в [руководстве по Rez](docs/guide/rez-skill-packages.md) описан запуск сервера в окружении с уже разрешёнными зависимостями.

## Как вызов попадает в нужный сеанс

![Путь запроса от клиента через шлюз до приложения](docs/assets/architecture/current-stack.svg)

Клиент отправляет запрос шлюзу. Шлюз находит зарегистрированный сеанс и передаёт вызов его адаптеру. Супервизор отслеживает, готов ли адаптер принимать вызовы. Сам инструмент работает в приложении. Если API приложения требует выполнения в главном потоке, адаптер передаёт работу этому потоку.

- **Выбор сеанса.** Если открыты два экземпляра приложения, прочитайте ресурс `gateway://instances`. Сверьте адрес `mcp_url` и состояние обработки вызовов для каждого экземпляра, прежде чем выбрать нужный. Подробнее в [руководстве по шлюзу](docs/guide/gateway.md).
- **Разработка клиента.** При работе по HTTP найдите инструмент через `POST /v1/search`. Когда он будет готов к вызову, передайте `tool_slug` и `arguments` в `POST /v1/call`. Для клиентов MCP сервер поддерживает Streamable HTTP и расширение Agent Skills. См. [описание REST API](docs/guide/rest-api-surface.md) и [руководство по навыкам MCP](docs/guide/mcp-skills-integration.md).
- **Повторение задачи.** Несколько шагов можно объединить в рабочий процесс. Если настроено постоянное хранилище, прерванное выполнение можно возобновить. Когда шаг доступен только в графическом интерфейсе, UI Control ограничивает ввод выбранным окном. См. руководства по [рабочим процессам](docs/guide/workflows.md) и [UI Control](docs/guide/ui-control-workflows.md).

## Поддерживаемые приложения

В каталоге есть адаптеры для Maya, Blender, Houdini, Photoshop, Unreal, Unity, Godot и RenderDoc. Чтобы посмотреть адаптеры, вошедшие в каталог вашей версии, выполните `dccmcp dcc-types`. В [полном списке приложений](docs/adapter-coverage.md) для каждого указаны соответствующий адаптер и доступные сведения об установке.

Чтобы сверить число записей в своей копии репозитория, выполните команду в корневом каталоге.

~~~bash
rg -c '^    tags: \[.*"adapter".*\]' dcc-mcp-catalog.yml
~~~

В [каталоге](dcc-mcp-catalog.yml) этой версии **49 записей об адаптерах**. Чтобы подключить приложение, разработанное внутри студии, воспользуйтесь [руководством по созданию адаптера](docs/guide/new-adapter-onboarding.md).

## Участие в разработке

Из корня репозитория выполните следующие команды. Они используют закреплённые в проекте версии инструментов, собирают пакет для разработки и запускают тесты Python и Rust.

~~~bash
vx just install
vx just dev
vx just test
vx just test-rust
~~~

После изменений в документации выполните `vx just docs-check`. Если изменилась команда или поле ответа, обновите пример во всех языковых версиях README. Пишите пояснения так, как принято в технических текстах на соответствующем языке, и следите за тем, чтобы исполняемые примеры совпадали. См. [правила участия в проекте](CONTRIBUTING.md) и [указатель документации](docs/guide/INDEX.md).

dcc-mcp-core распространяется по [лицензии MIT](LICENSE). [Сайт проекта](https://dcc-mcp.github.io/) · [Примеры проектов](https://dcc-mcp.github.io/showcase) · [Выпуски](https://github.com/dcc-mcp/dcc-mcp-core/releases) · [Реестр MCP](https://registry.modelcontextprotocol.io/?q=io.github.dcc-mcp%2Fdcc-mcp-core)
