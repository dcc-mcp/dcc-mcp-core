# dcc-mcp-core

Reliez un agent d’IA ou un pipeline de production aux logiciels de création de votre studio. Avec dcc-mcp-core, un client peut chercher un outil dans Maya, Blender ou Photoshop et l’exécuter dans la session où la scène ou le document est ouvert. Le moteur commun, écrit en Rust et en Python, prend en charge MCP (Model Context Protocol) et REST.

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core?label=PyPI)](https://pypi.org/project/dcc-mcp-core/) [![CI](https://img.shields.io/github/actions/workflow/status/dcc-mcp/dcc-mcp-core/ci.yml?branch=main&label=CI)](https://github.com/dcc-mcp/dcc-mcp-core/actions/workflows/ci.yml) [![MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

[English](README.md) · [简体中文](README_zh.md) · [日本語](README_ja.md) · [한국어](README_ko.md) · [Español](README_es.md) · **Français** · [Deutsch](README_de.md) · [Português (Brasil)](README_pt-BR.md) · [Русский](README_ru.md) · [Türkçe](README_tr.md)

## Skills over MCP

Avec **Skills over MCP** et MCP `2026-07-28`, les clients compatibles peuvent découvrir les Agent Skills sur un point d’accès DCC qui annonce l’extension, puis lire `SKILL.md` et les fichiers associés à la demande. Les clients et scripts existants peuvent continuer à utiliser `dccmcp` via l’API REST de la passerelle. [Guide d’intégration](docs/guide/mcp-skills-integration.md)

## Trouver un outil dans une application ouverte

Installez l’[outil en ligne de commande `dccmcp`](https://github.com/dcc-mcp/dcc-mcp-cli), puis activez l’[adaptateur DCC-MCP](docs/adapter-coverage.md) de votre application. Si vous travaillez avec un agent, le [Skill `dcc-mcp`](https://clawhub.ai/loonghao/skills/dcc-mcp) lui indique comment utiliser cette même interface. Pour chercher un outil qui dresse la liste des objets d’une scène Maya, exécutez ces deux commandes en lecture seule.

~~~bash
dccmcp list
dccmcp search --query "list objects" --dcc-type maya
~~~

`list` affiche les sessions enregistrées auprès de la passerelle. Quand `search` trouve un outil, le champ `tool_slug` en donne l’identifiant. Suivez l’indication de `next_step` pour charger le Skill requis, consulter les arguments attendus ou appeler directement l’outil. La recherche laisse la scène intacte.

Si Maya n’apparaît pas dans la liste, vérifiez que son adaptateur est actif et que `dccmcp` utilise la bonne passerelle. La commande `dccmcp doctor` permet d’examiner la connexion. En cas d’échec d’une requête, conservez son `request_id` pour la retrouver dans les journaux. Consultez la [référence de la CLI](docs/guide/cli-reference.md) pour les profils de connexion et le [guide de déploiement à distance](docs/guide/remote-server.md) pour les postes distants.

## Démarrer un serveur de Skills en Python

Vous pouvez essayer le serveur sans lancer de logiciel de création. Clonez le dépôt, placez-vous à sa racine et installez le paquet Python dans un environnement virtuel.

~~~bash
python -m pip install --upgrade "dcc-mcp-core>=0.20.34"
~~~

Exécutez le script Python ci-dessous. Il met à disposition l’[exemple de gestion des calques Photoshop](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers) sur un port local libre. Connectez votre client MCP à l’URL affichée dans le terminal. Appuyez sur Ctrl+C dans ce terminal pour arrêter le serveur.

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

Appelez `server/discover` sur le point d’accès DCC affiché et vérifiez qu’il annonce `io.modelcontextprotocol/skills`. Utilisez ensuite `skills/list` pour trouver `commercial-2d-layers`. Les anciens clients MCP peuvent trouver le paquet avec `search_skills`.

L’exemple illustre la structure d’un outil. [`SKILL.md`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/SKILL.md) désigne l’application cible et renvoie à [`tools.yaml`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/tools.yaml). Ce fichier déclare `summarize_layers` comme outil de lecture seule et définit son argument facultatif `document`. Le [script Python](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/scripts/summarize_layers.py) renvoie un résumé prédéfini. Pour lire un document ouvert, implémentez ce traitement dans l’adaptateur de l’application. Vous pouvez vérifier la déclaration du paquet avec cette commande.

~~~bash
dccmcp lint examples/rez-skills/commercial_2d_layers/skills
~~~

Pour créer vos propres outils, consultez le [guide des Skills](docs/guide/skills.md). Vous trouverez des paquets prêts à l’emploi sur la [Marketplace](https://dcc-mcp.github.io/marketplace). Si votre studio distribue ses outils avec Rez, le [guide Rez](docs/guide/rez-skill-packages.md) explique comment lancer le serveur dans un environnement de paquets résolu.

## Comment un appel arrive dans la bonne session

![Trajet d’un appel du client à l’application via la passerelle](docs/assets/architecture/current-stack.svg)

Le client envoie sa requête à la passerelle. Celle-ci retrouve la session dans son registre et transmet l’appel à l’adaptateur correspondant. Le superviseur suit l’état de l’adaptateur pour savoir s’il est prêt. L’adaptateur exécute l’outil dans l’application et confie le travail au thread principal lorsque l’API hôte l’exige.

- **Choisir une session.** Quand deux instances de la même application sont ouvertes, consultez `gateway://instances`. L’adresse `mcp_url` et l’état de chaque instance permettent de choisir celle qui recevra l’appel. Voir le [guide de la passerelle](docs/guide/gateway.md).
- **Écrire un client.** En HTTP, cherchez l’outil avec `POST /v1/search`. Quand il est prêt, envoyez `tool_slug` et `arguments` à `POST /v1/call`. Les clients MCP peuvent utiliser Streamable HTTP et l’extension Agent Skills. Voir l’[API REST](docs/guide/rest-api-surface.md) et le [guide MCP Skills](docs/guide/mcp-skills-integration.md).
- **Répéter une tâche.** Regroupez les étapes dans un workflow et configurez un stockage persistant pour pouvoir reprendre une exécution interrompue. Si une étape exige de passer par l’interface graphique, UI Control limite les entrées à la fenêtre ciblée. Voir les guides [Workflows](docs/guide/workflows.md) et [UI Control](docs/guide/ui-control-workflows.md).

## Applications prises en charge

Le catalogue comprend notamment Maya, Blender, Houdini, Photoshop, Unreal, Unity, Godot et RenderDoc. Pour consulter les adaptateurs répertoriés dans votre version, exécutez `dccmcp dcc-types`. La [liste complète des applications](docs/adapter-coverage.md) indique l’adaptateur correspondant et les informations d’installation disponibles.

Pour vérifier le nombre d’entrées dans votre copie du dépôt, lancez cette commande à la racine.

~~~bash
rg -c '^    tags: \[.*"adapter".*\]' dcc-mcp-catalog.yml
~~~

Le [catalogue](dcc-mcp-catalog.yml) de cette version contient **49 entrées d’adaptateurs**. Pour intégrer une application développée au studio, suivez le [guide de création d’un adaptateur](docs/guide/new-adapter-onboarding.md).

## Contribuer

Depuis la racine du dépôt, utilisez les versions d’outils prévues par le projet pour construire le paquet de développement et exécuter les tests Python et Rust.

~~~bash
vx just install
vx just dev
vx just test
vx just test-rust
~~~

Après une modification de la documentation, exécutez `vx just docs-check`. Si une commande ou un champ de réponse change, mettez à jour les exemples dans chaque README. Gardez les mêmes exemples exécutables et adaptez les explications aux usages de chaque langue. Consultez le [guide de contribution](CONTRIBUTING.md) et l’[index de la documentation](docs/guide/INDEX.md).

dcc-mcp-core est distribué sous [licence MIT](LICENSE). [Site du projet](https://dcc-mcp.github.io/) · [Exemples de réalisations](https://dcc-mcp.github.io/showcase) · [Versions](https://github.com/dcc-mcp/dcc-mcp-core/releases) · [Registre MCP](https://registry.modelcontextprotocol.io/?q=io.github.dcc-mcp%2Fdcc-mcp-core)
