# dcc-mcp-core

Mit dcc-mcp-core greifen KI-Agenten und Produktionspipelines auf die Kreativsoftware im Studio zu. Ein Client kann etwa in Maya, Blender oder Photoshop nach einem Werkzeug suchen und es in der Sitzung ausführen, in der die betreffende Szene oder Datei geöffnet ist. Die gemeinsame Laufzeitumgebung ist in Rust und Python geschrieben und unterstützt das Model Context Protocol (MCP) sowie REST.

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core?label=PyPI)](https://pypi.org/project/dcc-mcp-core/) [![CI](https://img.shields.io/github/actions/workflow/status/dcc-mcp/dcc-mcp-core/ci.yml?branch=main&label=CI)](https://github.com/dcc-mcp/dcc-mcp-core/actions/workflows/ci.yml) [![MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

[English](README.md) · [简体中文](README_zh.md) · [日本語](README_ja.md) · [한국어](README_ko.md) · [Español](README_es.md) · [Français](README_fr.md) · **Deutsch** · [Português (Brasil)](README_pt-BR.md) · [Русский](README_ru.md) · [Türkçe](README_tr.md)

## Skills over MCP

Kompatible Clients können mit **Skills over MCP** und MCP `2026-07-28` Agent Skills an einem DCC-Endpunkt entdecken, der die Erweiterung ausweist. `SKILL.md` und Begleitdateien lassen sich bei Bedarf abrufen. Bestehende Clients und Skripte können weiterhin `dccmcp` über die REST-API des Gateways verwenden. [Integrationsleitfaden](docs/guide/mcp-skills-integration.md)

## Ein Werkzeug in einer laufenden Anwendung finden

Installiere das [Kommandozeilenwerkzeug `dccmcp`](https://github.com/dcc-mcp/dcc-mcp-cli) und aktiviere in der Anwendung den passenden [DCC-MCP-Adapter](docs/adapter-coverage.md). Wenn du mit einem Agenten arbeitest, zeigt ihm der [Skill `dcc-mcp`](https://clawhub.ai/loonghao/skills/dcc-mcp), wie er dieselbe CLI nutzt. Mit den folgenden beiden Abfragen suchst du zum Beispiel nach einem Werkzeug, das die Objekte einer Maya-Szene auflistet.

~~~bash
dccmcp list
dccmcp search --query "list objects" --dcc-type maya
~~~

`list` zeigt die beim Gateway registrierten Sitzungen. Bei einem Suchtreffer enthält `tool_slug` die Kennung des Werkzeugs. Das Feld `next_step` gibt an, ob du den zugehörigen Skill laden, die erwarteten Argumente nachschlagen oder das Werkzeug direkt aufrufen kannst. Die Suche verändert die Szene nicht.

Fehlt Maya in der Liste, prüfe, ob der Adapter läuft und die CLI das richtige Gateway verwendet. Mit `dccmcp doctor` kannst du die Verbindung untersuchen. Schlägt eine Anfrage fehl, notiere dir ihre `request_id`, um sie im Gateway-Protokoll wiederzufinden. Die [CLI-Referenz](docs/guide/cli-reference.md) beschreibt Verbindungsprofile. Die [Anleitung für entfernte Arbeitsplätze](docs/guide/remote-server.md) erklärt die Einrichtung auf einem anderen Rechner.

## Einen Skill-Server mit Python starten

Den Server kannst du ausprobieren, ohne eine DCC-Anwendung zu öffnen. Klone das Repository, wechsle ins Stammverzeichnis und installiere das Python-Paket in einer virtuellen Umgebung.

~~~bash
python -m pip install dcc-mcp-core
~~~

Führe den folgenden Code als Python-Skript aus. Er stellt das [Beispiel für Photoshop-Ebenen](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers) an einem freien lokalen Port bereit. Verbinde deinen MCP-Client mit der angezeigten URL. Der Server läuft weiter, bis du ihn im Terminal mit Strg+C beendest.

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

Rufe am ausgegebenen DCC-Endpunkt `server/discover` auf und prüfe, ob er `io.modelcontextprotocol/skills` meldet. Suche dann mit `skills/list` nach `commercial-2d-layers`. Ältere MCP-Clients können das Paket mit `search_skills` finden.

Am Beispiel lässt sich der Aufbau eines Werkzeugs nachvollziehen. In [`SKILL.md`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/SKILL.md) stehen die Zielanwendung und der Verweis auf [`tools.yaml`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/tools.yaml). Dort ist `summarize_layers` als lesendes Werkzeug mit dem optionalen Argument `document` deklariert. Das [Python-Skript](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/scripts/summarize_layers.py) liefert eine fest vorgegebene Zusammenfassung. Um ein tatsächlich geöffnetes Dokument auszulesen, implementierst du die Verarbeitung im Adapter der Anwendung. Die Paketdefinition prüfst du mit diesem Befehl.

~~~bash
dccmcp lint examples/rez-skills/commercial_2d_layers/skills
~~~

Der [Skills-Leitfaden](docs/guide/skills.md) erklärt, wie du eigene Werkzeuge definierst und prüfst. Vorhandene Pakete findest du im [Marketplace](https://dcc-mcp.github.io/marketplace). Falls dein Studio Werkzeuge mit Rez verteilt, zeigt der [Rez-Leitfaden](docs/guide/rez-skill-packages.md), wie du den Server in einer aufgelösten Paketumgebung startest.

## Wie ein Aufruf die richtige Sitzung erreicht

![Weg eines Aufrufs vom Client über das Gateway zur DCC-Anwendung](docs/assets/architecture/current-stack.svg)

Der Client sendet seine Anfrage an das Gateway. Das Gateway findet die registrierte Sitzung und leitet den Aufruf an deren Adapter weiter. Der Supervisor überwacht, ob der Adapter bereit ist. Der Adapter führt das Werkzeug in der Anwendung aus und übergibt die Arbeit an den Hauptthread, wenn die API der Anwendung das verlangt.

- **Eine Sitzung auswählen.** Laufen zwei Instanzen derselben Anwendung, lies `gateway://instances` aus. Prüfe für jeden Eintrag die `mcp_url` und den Status der Aufrufverarbeitung, bevor du das Ziel auswählst. Einzelheiten stehen im [Gateway-Leitfaden](docs/guide/gateway.md).
- **Einen Client anbinden.** Über HTTP suchst du mit `POST /v1/search` nach dem Werkzeug. Sobald es bereit ist, sendest du `tool_slug` und `arguments` an `POST /v1/call`. MCP-Clients können Streamable HTTP und die Erweiterung Agent Skills nutzen. Siehe [REST-API](docs/guide/rest-api-surface.md) und [MCP-Skills-Leitfaden](docs/guide/mcp-skills-integration.md).
- **Eine Aufgabe wiederholen.** Fasse mehrere Schritte zu einem Workflow zusammen. Mit persistentem Speicher lässt sich ein unterbrochener Lauf fortsetzen. Für Schritte, die nur über die Bedienoberfläche möglich sind, beschränkt UI Control die Eingabe auf ein bestimmtes Fenster. Siehe [Workflows](docs/guide/workflows.md) und [UI Control](docs/guide/ui-control-workflows.md).

## Unterstützte Anwendungen

Der Katalog enthält unter anderem Maya, Blender, Houdini, Photoshop, Unreal, Unity, Godot und RenderDoc. Mit `dccmcp dcc-types` siehst du, welche Adapter im Katalog deiner Version stehen. Die [vollständige Anwendungsliste](docs/adapter-coverage.md) nennt den jeweils zugehörigen Adapter und die verfügbaren Installationsdaten.

Die Zahl der Einträge in deinem Checkout kannst du im Stammverzeichnis des Repositorys überprüfen.

~~~bash
rg -c '^    tags: \[.*"adapter".*\]' dcc-mcp-catalog.yml
~~~

Der [Katalog](dcc-mcp-catalog.yml) dieser Version umfasst **49 Adaptereinträge**. Wie du eine studioeigene Anwendung anschließt, beschreibt die [Anleitung zur Adapterentwicklung](docs/guide/new-adapter-onboarding.md).

## Mitwirken

Die folgenden Befehle verwenden die im Projekt festgelegten Werkzeugversionen. Führe sie im Stammverzeichnis aus, um das Entwicklungspaket zu bauen und die Python- und Rust-Tests zu starten.

~~~bash
vx just install
vx just dev
vx just test
vx just test-rust
~~~

Prüfe Änderungen an der Dokumentation mit `vx just docs-check`. Ändert sich ein Befehl oder ein Antwortfeld, passe das Beispiel in allen README-Dateien an. Formuliere die Erklärungen so, wie sie in der jeweiligen Sprache üblich sind, und halte die ausführbaren Beispiele auf demselben Stand. Siehe [Hinweise für Mitwirkende](CONTRIBUTING.md) und [Dokumentationsübersicht](docs/guide/INDEX.md).

dcc-mcp-core steht unter der [MIT-Lizenz](LICENSE). [Projektseite](https://dcc-mcp.github.io/) · [Anwendungsbeispiele](https://dcc-mcp.github.io/showcase) · [Veröffentlichungen](https://github.com/dcc-mcp/dcc-mcp-core/releases) · [MCP Registry](https://registry.modelcontextprotocol.io/?q=io.github.dcc-mcp%2Fdcc-mcp-core)
