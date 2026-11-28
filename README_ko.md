# dcc-mcp-core

스튜디오에서 쓰는 제작 도구를 AI 에이전트나 제작 파이프라인에 연결하세요. dcc-mcp-core를 사용하면 클라이언트가 Maya, Blender, Photoshop에서 도구를 찾고, 작업할 장면이나 문서가 열려 있는 세션에서 그 도구를 실행할 수 있습니다. Rust와 Python으로 구현한 공통 런타임은 MCP(Model Context Protocol)와 REST를 지원합니다.

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core?label=PyPI)](https://pypi.org/project/dcc-mcp-core/) [![CI](https://img.shields.io/github/actions/workflow/status/dcc-mcp/dcc-mcp-core/ci.yml?branch=main&label=CI)](https://github.com/dcc-mcp/dcc-mcp-core/actions/workflows/ci.yml) [![MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

[English](README.md) · [简体中文](README_zh.md) · [日本語](README_ja.md) · **한국어** · [Español](README_es.md) · [Français](README_fr.md) · [Deutsch](README_de.md) · [Português (Brasil)](README_pt-BR.md) · [Русский](README_ru.md) · [Türkçe](README_tr.md)

## Skills over MCP

**Skills over MCP**와 MCP `2026-07-28`을 지원하는 클라이언트는 확장을 알리는 DCC 엔드포인트에서 Agent Skills를 찾고, 필요할 때 `SKILL.md`와 관련 파일을 읽을 수 있습니다. 기존 클라이언트와 스크립트는 게이트웨이의 REST API를 통해 계속 `dccmcp`를 사용할 수 있습니다. [통합 가이드](docs/guide/mcp-skills-integration.md)

## 실행 중인 앱에서 도구 찾기

[`dccmcp` CLI](https://github.com/dcc-mcp/dcc-mcp-cli)를 설치하고 작업할 앱에서 해당 [DCC-MCP 어댑터](docs/adapter-coverage.md)를 활성화합니다. 에이전트로 작업한다면 [`dcc-mcp` Skill](https://clawhub.ai/loonghao/skills/dcc-mcp)을 설치해 같은 CLI를 사용하도록 할 수 있습니다. 예를 들어 Maya 장면의 오브젝트 목록을 읽는 도구를 찾으려면 다음 두 명령을 실행하세요.

~~~bash
dccmcp list
dccmcp search --query "list objects" --dcc-type maya
~~~

`list`는 게이트웨이에 등록된 세션을 보여 줍니다. `search`로 도구를 찾으면 결과에 도구 식별자 `tool_slug`가 표시됩니다. 이어서 `next_step`을 확인하세요. 필요한 Skill을 먼저 불러올지, 인수를 확인할지, 도구를 바로 호출할지 알려 줍니다. 검색만으로 장면이 바뀌지는 않습니다.

목록에 Maya가 없다면 어댑터가 실행 중인지, CLI가 올바른 게이트웨이에 연결되어 있는지 확인합니다. `dccmcp doctor`로 연결 상태를 점검할 수 있습니다. 요청이 실패하면 `request_id`를 기록해 두세요. 게이트웨이 로그에서 해당 요청을 찾을 때 쓰는 값입니다. 접속 프로필 설정은 [CLI 레퍼런스](docs/guide/cli-reference.md), 다른 워크스테이션에 배포하는 방법은 [원격 서버 가이드](docs/guide/remote-server.md)를 참고하세요.

## Python으로 Skill 서버 실행하기

제작 앱을 열지 않고도 서버를 시험해 볼 수 있습니다. 저장소를 복제한 뒤 루트 디렉터리에서 Python 가상 환경을 활성화하고 패키지를 설치합니다.

~~~bash
python -m pip install --upgrade "dcc-mcp-core>=0.20.34"
~~~

아래 코드를 Python 스크립트로 실행하면 [Photoshop 레이어 예제](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers)를 제공하는 서버가 로컬의 빈 포트에서 시작됩니다. 터미널에 출력된 URL로 MCP 클라이언트를 연결하세요. 스크립트를 실행한 터미널에서 Ctrl+C를 누르면 서버가 종료됩니다.

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

터미널에 표시된 DCC 엔드포인트에 `server/discover`를 호출해 `io.modelcontextprotocol/skills`를 제공하는지 확인하세요. 그런 다음 `skills/list`로 `commercial-2d-layers`를 찾을 수 있습니다. 구형 MCP 클라이언트는 `search_skills`로 같은 패키지를 찾을 수 있습니다.

이 예제에서는 도구 하나를 패키지로 구성하는 방법을 볼 수 있습니다. [`SKILL.md`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/SKILL.md)에 대상 앱과 도구 정의 파일을 지정합니다. [`tools.yaml`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/tools.yaml)에는 `summarize_layers`가 읽기 전용이며 `document` 인수는 생략할 수 있다고 선언되어 있습니다. 예제의 [Python 스크립트](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/scripts/summarize_layers.py)는 미리 정해 둔 요약을 반환합니다. 실제로 열려 있는 문서를 읽으려면 어댑터에 해당 처리 로직을 구현하세요. 다음 명령으로 패키지 정의를 검사할 수 있습니다.

~~~bash
dccmcp lint examples/rez-skills/commercial_2d_layers/skills
~~~

직접 도구를 만들 때는 [Skills 가이드](docs/guide/skills.md)를 참고하고, 이미 만들어진 도구는 [Marketplace](https://dcc-mcp.github.io/marketplace)에서 찾아보세요. Rez로 도구를 배포하는 스튜디오라면 [Rez 가이드](docs/guide/rez-skill-packages.md)에서 패키지 의존성을 해결한 환경으로 서버를 시작하는 방법을 확인할 수 있습니다.

## 호출이 대상 세션에 도달하는 과정

![클라이언트에서 게이트웨이를 거쳐 제작 앱까지 이어지는 호출 경로](docs/assets/architecture/current-stack.svg)

클라이언트가 요청을 보내면 게이트웨이는 등록된 세션 중 대상을 찾아 해당 어댑터로 호출을 전달합니다. 감독 프로세스는 어댑터가 요청을 받을 준비가 되었는지 추적합니다. 도구는 앱 안에서 실행되며, 호스트 API가 요구하면 어댑터가 작업을 메인 스레드로 보냅니다.

- **세션 선택.** 같은 앱을 두 개 열어 두었다면 `gateway://instances`에서 각 세션의 `mcp_url`과 호출을 받을 수 있는 상태인지 확인합니다. 자세한 내용은 [게이트웨이 가이드](docs/guide/gateway.md)를 참고하세요.
- **클라이언트 구현.** HTTP로 연동할 때는 `POST /v1/search`로 도구를 찾고, 도구가 준비되면 `tool_slug`와 `arguments`를 `POST /v1/call`로 보냅니다. MCP 클라이언트는 Streamable HTTP와 Agent Skills 확장을 사용할 수 있습니다. [REST API](docs/guide/rest-api-surface.md)와 [MCP Skills 가이드](docs/guide/mcp-skills-integration.md)에 세부 사항이 나와 있습니다.
- **작업 반복.** 여러 단계를 워크플로로 묶고 영구 저장소를 설정하면 중단된 작업을 재개할 수 있습니다. 화면에서만 가능한 조작에는 입력 대상을 특정 창으로 제한하는 UI Control을 사용합니다. [워크플로](docs/guide/workflows.md)와 [UI Control](docs/guide/ui-control-workflows.md)을 참고하세요.

## 지원하는 앱

Maya, Blender, Houdini, Photoshop, Unreal, Unity, Godot, RenderDoc 등이 카탈로그에 등록되어 있습니다. `dccmcp dcc-types`로 현재 릴리스의 어댑터를 확인할 수 있습니다. [전체 앱 목록](docs/adapter-coverage.md)에는 앱별 어댑터와 공개된 설치 정보가 정리되어 있습니다.

체크아웃한 버전의 항목 수는 저장소 루트에서 다음 명령으로 확인하세요.

~~~bash
rg -c '^    tags: \[.*"adapter".*\]' dcc-mcp-catalog.yml
~~~

이 버전의 [카탈로그](dcc-mcp-catalog.yml)에는 **어댑터 49개**가 등록되어 있습니다. 스튜디오에서 자체 개발한 앱을 연결하려면 [어댑터 개발 가이드](docs/guide/new-adapter-onboarding.md)를 참고하세요.

## 개발에 참여하기

저장소 루트에서 다음 명령을 실행하면 프로젝트가 지정한 도구 버전으로 개발용 패키지를 빌드하고 Python 및 Rust 테스트를 실행할 수 있습니다.

~~~bash
vx just install
vx just dev
vx just test
vx just test-rust
~~~

문서를 수정했다면 `vx just docs-check`를 실행하세요. 명령이나 응답 필드가 바뀌면 각 언어의 README에 있는 예제도 함께 갱신합니다. 설명은 해당 언어의 독자에게 자연스럽게 쓰고 실행 예제의 내용은 동일하게 유지합니다. 자세한 내용은 [기여 가이드](CONTRIBUTING.md)와 [문서 목록](docs/guide/INDEX.md)을 참고하세요.

dcc-mcp-core는 [MIT 라이선스](LICENSE)로 배포합니다. [프로젝트 홈페이지](https://dcc-mcp.github.io/) · [활용 사례](https://dcc-mcp.github.io/showcase) · [릴리스](https://github.com/dcc-mcp/dcc-mcp-core/releases) · [MCP Registry](https://registry.modelcontextprotocol.io/?q=io.github.dcc-mcp%2Fdcc-mcp-core)
