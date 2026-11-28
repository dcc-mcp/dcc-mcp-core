# dcc-mcp-core

dcc-mcp-core ile bir yapay zekâ ajanını veya üretim hattını stüdyonuzda kullandığınız tasarım ve içerik üretim uygulamalarına bağlayabilirsiniz. İstemci, Maya, Blender ya da Photoshop’ta bir araç bulur ve aracı ilgili sahnenin veya belgenin açık olduğu oturumda çalıştırır. Rust ve Python ile yazılan ortak çalışma ortamı hem MCP’yi (Model Context Protocol) hem de REST’i destekler.

[![PyPI](https://img.shields.io/pypi/v/dcc-mcp-core?label=PyPI)](https://pypi.org/project/dcc-mcp-core/) [![CI](https://img.shields.io/github/actions/workflow/status/dcc-mcp/dcc-mcp-core/ci.yml?branch=main&label=CI)](https://github.com/dcc-mcp/dcc-mcp-core/actions/workflows/ci.yml) [![MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

[English](README.md) · [简体中文](README_zh.md) · [日本語](README_ja.md) · [한국어](README_ko.md) · [Español](README_es.md) · [Français](README_fr.md) · [Deutsch](README_de.md) · [Português (Brasil)](README_pt-BR.md) · [Русский](README_ru.md) · **Türkçe**

## Skills over MCP

**Skills over MCP** ve MCP `2026-07-28` desteği olan istemciler, uzantıyı duyuran bir DCC uç noktasında Agent Skills paketlerini bulabilir ve gerektiğinde `SKILL.md` ile ilgili dosyaları okuyabilir. Mevcut istemciler ve betikler, ağ geçidinin REST API’si üzerinden `dccmcp` kullanmaya devam edebilir. [Entegrasyon kılavuzu](docs/guide/mcp-skills-integration.md)

## Açık bir uygulamada araç bulma

[`dccmcp` komut satırı aracını](https://github.com/dcc-mcp/dcc-mcp-cli) kurun ve kullanacağınız uygulamada ilgili [DCC-MCP bağdaştırıcısını](docs/adapter-coverage.md) etkinleştirin. Bir ajanla çalışıyorsanız [`dcc-mcp` Skill paketi](https://clawhub.ai/loonghao/skills/dcc-mcp) ajana aynı komut satırı aracını nasıl kullanacağını gösterir. Örneğin, Maya sahnesindeki nesneleri listeleyen bir araç aramak için aşağıdaki iki komutu çalıştırın.

~~~bash
dccmcp list
dccmcp search --query "list objects" --dcc-type maya
~~~

`list`, ağ geçidine kayıtlı oturumları gösterir. Arama bir araç bulduğunda `tool_slug` alanında aracın kimliği yer alır. Sonraki adım için `next_step` alanına bakın. Burada ilgili Skill paketini yüklemeniz, parametreleri incelemeniz veya aracı doğrudan çağırmanız istenir. Arama sahneyi değiştirmez.

Listede Maya’yı göremiyorsanız bağdaştırıcının çalıştığını ve komut satırı aracının doğru ağ geçidine bağlandığını kontrol edin. Bağlantıyı incelemek için `dccmcp doctor` komutunu kullanabilirsiniz. Bir istek başarısız olursa ağ geçidi günlüklerinde bulabilmek için `request_id` değerini kaydedin. Bağlantı profilleri için [komut satırı kılavuzuna](docs/guide/cli-reference.md), başka bir iş istasyonuna kurulum için [uzak sunucu kılavuzuna](docs/guide/remote-server.md) bakın.

## Python ile bir Skill sunucusu çalıştırma

Sunucuyu denemek için bir tasarım uygulaması açmanız gerekmez. Depoyu klonlayın, kök dizine geçin ve Python paketini bir sanal ortama kurun.

~~~bash
python -m pip install dcc-mcp-core
~~~

Aşağıdaki kodu bir Python betiği olarak çalıştırın. Betik, [Photoshop katman örneğini](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers) bilgisayarınızdaki boş bir bağlantı noktasından sunar. MCP istemcinizi terminale yazdırılan URL’ye bağlayın. Sunucuyu durdurmak için betiği çalıştırdığınız terminalde Ctrl+C tuşlarına basın.

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

Terminalde gösterilen DCC uç noktasında `server/discover` çağrısı yapın ve `io.modelcontextprotocol/skills` uzantısının duyurulduğunu doğrulayın. Ardından `skills/list` ile `commercial-2d-layers` paketini bulun. Eski MCP istemcileri aynı paketi `search_skills` ile bulabilir.

Bu örnekte bir aracın hangi dosyalardan oluştuğunu görebilirsiniz. [`SKILL.md`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/SKILL.md) hedef uygulamayı belirtir ve [`tools.yaml`](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/tools.yaml) dosyasını işaret eder. Bu dosyada `summarize_layers` salt okunur bir araç olarak tanımlanır. `document` parametresi isteğe bağlıdır. Örnekteki [Python betiği](examples/rez-skills/commercial_2d_layers/skills/commercial-2d-layers/scripts/summarize_layers.py) önceden hazırlanmış bir özet döndürür. Açık bir belgeyi okumak için bu işlemi uygulamanın bağdaştırıcısında gerçekleştirmeniz gerekir. Paket tanımını aşağıdaki komutla doğrulayabilirsiniz.

~~~bash
dccmcp lint examples/rez-skills/commercial_2d_layers/skills
~~~

Kendi araçlarınızı tanımlayıp doğrulamak için [Skill kılavuzuna](docs/guide/skills.md) bakın. Hazır paketleri [Marketplace](https://dcc-mcp.github.io/marketplace) üzerinden bulabilirsiniz. Stüdyonuz araçları Rez ile dağıtıyorsa [Rez kılavuzu](docs/guide/rez-skill-packages.md), paket bağımlılıklarının çözümlendiği ortamda sunucuyu nasıl başlatacağınızı anlatır.

## Çağrı doğru oturuma nasıl ulaşır?

İstemci isteğini ağ geçidine gönderir. Ağ geçidi kayıtlı oturumu bulur ve çağrıyı o oturumun bağdaştırıcısına yönlendirir. Gözetmen süreç, bağdaştırıcının hazır olup olmadığını izler. Araç uygulamanın içinde çalışır. Uygulamanın API’si gerektiriyorsa bağdaştırıcı işi ana iş parçacığına aktarır.

![Çağrının istemciden ağ geçidi üzerinden uygulamaya izlediği yol](docs/assets/architecture/current-stack.svg)

- **Oturum seçimi.** Aynı uygulamanın iki örneği açıksa `gateway://instances` kaynağını okuyun. Hedefi seçmeden önce her kaydın `mcp_url` adresini ve çağrı almaya hazır olup olmadığını kontrol edin. Ayrıntılar için [ağ geçidi kılavuzuna](docs/guide/gateway.md) bakın.
- **İstemci geliştirme.** HTTP üzerinden çalışıyorsanız aracı `POST /v1/search` ile bulun. Araç hazır olduğunda `tool_slug` ve `arguments` alanlarını `POST /v1/call` adresine gönderin. Sunucu, MCP istemcileri için Streamable HTTP ve Agent Skills uzantısını destekler. Ayrıntılar [REST API](docs/guide/rest-api-surface.md) ve [MCP Skills kılavuzunda](docs/guide/mcp-skills-integration.md) yer alır.
- **Bir işi tekrarlama.** Birden çok adımı bir iş akışında toplayabilirsiniz. Kalıcı depolama yapılandırıldığında yarıda kalan bir çalışmayı sürdürebilirsiniz. Bir adım yalnızca masaüstü arayüzünden gerçekleştirilebiliyorsa UI Control, girdiyi seçilen pencereyle sınırlar. [İş akışları](docs/guide/workflows.md) ve [UI Control](docs/guide/ui-control-workflows.md) kılavuzlarına bakın.

## Desteklenen uygulamalar

Katalogda Maya, Blender, Houdini, Photoshop, Unreal, Unity, Godot ve RenderDoc gibi uygulamalar yer alır. Kullandığınız sürümün kataloğundaki bağdaştırıcıları görmek için `dccmcp dcc-types` komutunu çalıştırın. [Uygulamaların tam listesinde](docs/adapter-coverage.md) her uygulamanın bağdaştırıcısını ve varsa kurulum bilgilerini bulabilirsiniz.

Elinizdeki sürümün kaç kayıt içerdiğini denetlemek için deponun kök dizininde şu komutu çalıştırın.

~~~bash
rg -c '^    tags: \[.*"adapter".*\]' dcc-mcp-catalog.yml
~~~

Bu sürümün [kataloğunda](dcc-mcp-catalog.yml) **49 bağdaştırıcı kaydı** bulunur. Stüdyonuzda geliştirilen bir uygulamayı bağlamak için [bağdaştırıcı geliştirme kılavuzunu](docs/guide/new-adapter-onboarding.md) izleyin.

## Katkıda bulunma

Aşağıdaki komutları deponun kök dizininde çalıştırarak projenin belirlediği araç sürümleriyle geliştirme paketini derleyebilir, Python ve Rust testlerini çalıştırabilirsiniz.

~~~bash
vx just install
vx just dev
vx just test
vx just test-rust
~~~

Belgeleri değiştirdiğinizde `vx just docs-check` komutunu çalıştırın. Bir komut veya yanıt alanı değişirse her dildeki README örneğini güncelleyin. Açıklamaları o dilin kullanımına göre yazın ve çalıştırılabilir örneklerin birbiriyle uyumlu kalmasını sağlayın. Ayrıntılar için [katkı kılavuzuna](CONTRIBUTING.md) ve [belge dizinine](docs/guide/INDEX.md) bakın.

dcc-mcp-core, [MIT lisansı](LICENSE) ile dağıtılır. [Proje sitesi](https://dcc-mcp.github.io/) · [Uygulama örnekleri](https://dcc-mcp.github.io/showcase) · [Sürümler](https://github.com/dcc-mcp/dcc-mcp-core/releases) · [MCP Registry](https://registry.modelcontextprotocol.io/?q=io.github.dcc-mcp%2Fdcc-mcp-core)
