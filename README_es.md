# dcc-mcp-core

Conecta un agente de IA o un proceso de producción con las aplicaciones de creación que ya usas en el estudio. Con dcc-mcp-core, un cliente puede buscar una herramienta en Maya, Blender o Photoshop y ejecutarla en la sesión que tiene abierta la escena o el documento. El entorno de ejecución común está escrito en Rust y Python y admite MCP (Model Context Protocol) y REST.

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core?label=PyPI)](https://pypi.org/project/dcc-mcp-core/) [![CI](https://img.shields.io/github/actions/workflow/status/dcc-mcp/dcc-mcp-core/ci.yml?branch=main&label=CI)](https://github.com/dcc-mcp/dcc-mcp-core/actions/workflows/ci.yml) [![MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

[English](README.md) · [简体中文](README_zh.md) · [日本語](README_ja.md) · [한국어](README_ko.md) · **Español** · [Français](README_fr.md) · [Deutsch](README_de.md) · [Português (Brasil)](README_pt-BR.md) · [Русский](README_ru.md) · [Türkçe](README_tr.md)

## Skills over MCP

Con **Skills over MCP** y MCP `2026-07-28`, los clientes compatibles pueden descubrir Agent Skills en un endpoint de DCC que anuncie la extensión y leer `SKILL.md` y sus archivos auxiliares cuando los necesiten. Los clientes y scripts existentes pueden seguir usando `dccmcp` a través de la API REST del gateway. [Guía de integración](docs/guide/mcp-skills-integration.md)

## Buscar una herramienta en una aplicación abierta

Instala la [interfaz de línea de comandos `dccmcp`](https://github.com/dcc-mcp/dcc-mcp-cli) y activa el [adaptador DCC-MCP](docs/adapter-coverage.md) de la aplicación. Si trabajas con un agente, el [Skill `dcc-mcp`](https://clawhub.ai/loonghao/skills/dcc-mcp) le indica cómo usar la misma interfaz. Por ejemplo, para buscar una herramienta que enumere los objetos de una escena de Maya, ejecuta estas dos consultas de solo lectura.

~~~bash
dccmcp list
dccmcp search --query "list objects" --dcc-type maya
~~~

`list` muestra las sesiones registradas en la pasarela. Cuando `search` encuentra una herramienta, el campo `tool_slug` contiene su identificador. Consulta `next_step` para saber si debes cargar el Skill, revisar los argumentos que acepta la herramienta o llamarla directamente. La búsqueda deja la escena intacta.

Si Maya no aparece en la lista, comprueba que el adaptador esté en ejecución y que la CLI apunte a la pasarela correcta. Usa `dccmcp doctor` para revisar la conexión. Si falla una petición, guarda su `request_id` para localizarla en los registros. Consulta la [referencia de la CLI](docs/guide/cli-reference.md) para configurar perfiles y la [guía de despliegue remoto](docs/guide/remote-server.md) para conectar otro equipo.

## Probar un servidor de Skills con Python

Puedes probar el servidor sin abrir una aplicación de creación. Clona el repositorio, sitúate en su directorio raíz e instala el paquete de Python en un entorno virtual.

~~~bash
python -m pip install --upgrade "dcc-mcp-core>=0.20.34"
~~~

Ejecuta el siguiente código como un script de Python. El servidor ofrece el [ejemplo de capas de Photoshop](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers) en un puerto local disponible. Conecta tu cliente MCP a la URL que aparece en la terminal. Cuando termines, pulsa Ctrl+C en esa terminal para cerrar el servidor.

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

Llama a `server/discover` en el endpoint DCC que se muestra en la terminal y comprueba si anuncia `io.modelcontextprotocol/skills`. Usa `skills/list` para encontrar `commercial-2d-layers`. Los clientes MCP antiguos pueden encontrar el paquete con `search_skills`.

El ejemplo muestra cómo se organiza una herramienta. [`SKILL.md`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/SKILL.md) identifica la aplicación y remite a [`tools.yaml`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/tools.yaml). Allí se declara `summarize_layers` como herramienta de solo lectura, con el argumento opcional `document`. El [script de Python](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/scripts/summarize_layers.py) devuelve un resumen de muestra predefinido. Para leer un documento abierto, implementa esa operación en el adaptador de la aplicación. Puedes validar la definición del paquete con este comando.

~~~bash
dccmcp lint examples/rez-skills/commercial_2d_layers/skills
~~~

La [guía de Skills](docs/guide/skills.md) explica cómo definir y validar herramientas propias. Para reutilizar paquetes, consulta el [Marketplace](https://dcc-mcp.github.io/marketplace). Si distribuyes herramientas con Rez, la [guía de Rez](docs/guide/rez-skill-packages.md) describe cómo iniciar un servidor en un entorno con las dependencias ya resueltas.

## Cómo llega una llamada a la sesión correcta

![Recorrido de una llamada desde el cliente hasta la aplicación a través de la pasarela](docs/assets/architecture/current-stack.svg)

El cliente envía la petición a la pasarela. Esta localiza la sesión registrada y dirige la llamada a su adaptador. El supervisor comprueba si el adaptador está listo para recibirla. La herramienta se ejecuta dentro de la aplicación y, cuando su API lo exige, el adaptador envía el trabajo al hilo principal.

- **Elegir una sesión.** Si tienes dos instancias de una aplicación abiertas, consulta `gateway://instances`. Comprueba la dirección `mcp_url` y el estado de cada instancia antes de elegir el destino. Consulta la [guía de la pasarela](docs/guide/gateway.md).
- **Crear un cliente.** Con HTTP, busca la herramienta mediante `POST /v1/search`. Cuando esté lista, envía su `tool_slug` y los `arguments` a `POST /v1/call`. Para clientes MCP, el servidor admite Streamable HTTP y la extensión Agent Skills. Consulta la [API REST](docs/guide/rest-api-surface.md) y la [guía de MCP Skills](docs/guide/mcp-skills-integration.md).
- **Repetir una tarea.** Agrupa los pasos en un flujo de trabajo y configura almacenamiento persistente para poder reanudar una ejecución interrumpida. Si un paso solo se puede realizar desde la interfaz gráfica, UI Control limita la entrada a la ventana elegida. Consulta las guías de [flujos de trabajo](docs/guide/workflows.md) y [UI Control](docs/guide/ui-control-workflows.md).

## Aplicaciones compatibles

El catálogo incluye Maya, Blender, Houdini, Photoshop, Unreal, Unity, Godot y RenderDoc, entre otras aplicaciones. Ejecuta `dccmcp dcc-types` para ver los adaptadores incluidos en el catálogo de tu versión. La [lista completa de aplicaciones](docs/adapter-coverage.md) indica qué adaptador corresponde a cada una y recoge los datos de instalación disponibles.

Para comprobar cuántas entradas contiene tu copia del repositorio, ejecuta este comando en el directorio raíz.

~~~bash
rg -c '^    tags: \[.*"adapter".*\]' dcc-mcp-catalog.yml
~~~

El [catálogo](dcc-mcp-catalog.yml) de esta versión contiene **49 entradas de adaptadores**. Si quieres conectar una aplicación propia del estudio, consulta la [guía de desarrollo de adaptadores](docs/guide/new-adapter-onboarding.md).

## Contribuir al proyecto

Desde el directorio raíz, usa las versiones de las herramientas fijadas por el proyecto para compilar el paquete de desarrollo y ejecutar las pruebas de Python y Rust.

~~~bash
vx just install
vx just dev
vx just test
vx just test-rust
~~~

Si modificas la documentación, ejecuta `vx just docs-check`. Cuando cambie un comando o un campo de respuesta, actualiza el ejemplo en todos los README. Redacta las explicaciones según los usos de cada idioma y mantén sincronizados los ejemplos ejecutables. Consulta la [guía de contribución](CONTRIBUTING.md) y el [índice de la documentación](docs/guide/INDEX.md).

dcc-mcp-core se distribuye bajo la [licencia MIT](LICENSE). [Sitio del proyecto](https://dcc-mcp.github.io/) · [Galería de proyectos](https://dcc-mcp.github.io/showcase) · [Versiones](https://github.com/dcc-mcp/dcc-mcp-core/releases) · [Registro MCP](https://registry.modelcontextprotocol.io/?q=io.github.dcc-mcp%2Fdcc-mcp-core)
