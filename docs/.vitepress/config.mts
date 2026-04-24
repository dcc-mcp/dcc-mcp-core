import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'DCC-MCP-Core',
  description: 'Production-grade MCP + Skills foundation for AI-assisted DCC workflows',
  base: '/dcc-mcp-core/',
  cleanUrls: true,
  lastUpdated: true,

  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/dcc-mcp-core/logo.svg' }],
  ],

  locales: {
    root: {
      label: 'English',
      lang: 'en-US',
      themeConfig: {
        nav: [
          { text: 'Guide', link: '/guide/what-is-dcc-mcp-core' },
          { text: 'API', link: '/api/models' },
          {
            text: 'v0.14.9',
            items: [
              { text: 'Changelog', link: 'https://github.com/loonghao/dcc-mcp-core/blob/main/CHANGELOG.md' },
              { text: 'PyPI', link: 'https://pypi.org/project/dcc-mcp-core/' },
            ]
          }
        ],
        sidebar: {
          '/guide/': [
            {
              text: 'Introduction',
              items: [
                { text: 'What is DCC-MCP-Core?', link: '/guide/what-is-dcc-mcp-core' },
                { text: 'Getting Started', link: '/guide/getting-started' },
              ]
            },
            {
              text: 'MCP + Skills System',
              items: [
                { text: 'MCP Integration Guide', link: '/guide/mcp-skills-integration' },
                { text: 'Skills System', link: '/guide/skills' },
                { text: 'Skill Scopes & Policies', link: '/guide/skill-scopes-policies' },
                { text: 'Gateway Election', link: '/guide/gateway-election' },
                { text: 'Gateway', link: '/guide/gateway' },
                { text: 'Remote Server', link: '/guide/remote-server' },
                { text: 'Production Deployment', link: '/guide/production-deployment' },
              ]
            },
            {
              text: 'Core Concepts',
              items: [
                { text: 'Actions & Registry', link: '/guide/actions' },
                { text: 'Event System', link: '/guide/events' },
                { text: 'MCP Protocols', link: '/guide/protocols' },
                { text: 'Naming Actions & Tools', link: '/guide/naming' },
                { text: 'Transport Layer', link: '/guide/transport' },
                { text: 'Capabilities', link: '/guide/capabilities' },
                { text: 'Prompts', link: '/guide/prompts' },
              ]
            },
            {
              text: 'Advanced',
              items: [
                { text: 'Architecture', link: '/guide/architecture' },
                { text: 'Custom Skills', link: '/guide/custom-actions' },
                { text: 'DCC Thread Safety', link: '/guide/dcc-thread-safety' },
                { text: 'Process Management', link: '/guide/process' },
                { text: 'Sandbox & Security', link: '/guide/sandbox' },
                { text: 'Shared Memory', link: '/guide/shm' },
                { text: 'Telemetry', link: '/guide/telemetry' },
                { text: 'Capture', link: '/guide/capture' },
                { text: 'USD Bridge', link: '/guide/usd' },
                { text: 'Artefacts', link: '/guide/artefacts' },
                { text: 'Job Persistence', link: '/guide/job-persistence' },
                { text: 'Scheduler', link: '/guide/scheduler' },
                { text: 'Workflows', link: '/guide/workflows' },
                { text: 'FAQ', link: '/guide/faq' },
              ]
            },
          ],
          '/api/': [
            {
              text: 'API Reference',
              items: [
                { text: 'Models', link: '/api/models' },
                { text: 'Actions', link: '/api/actions' },
                { text: 'Events', link: '/api/events' },
                { text: 'Skills', link: '/api/skills' },
                { text: 'Protocols', link: '/api/protocols' },
                { text: 'Transport', link: '/api/transport' },
                { text: 'HTTP Server', link: '/api/http' },
                { text: 'Process', link: '/api/process' },
                { text: 'Sandbox', link: '/api/sandbox' },
                { text: 'Shared Memory', link: '/api/shm' },
                { text: 'Telemetry', link: '/api/telemetry' },
                { text: 'Capture', link: '/api/capture' },
                { text: 'USD', link: '/api/usd' },
                { text: 'Utilities', link: '/api/utilities' },
                { text: 'Observability', link: '/api/observability' },
                { text: 'Resources', link: '/api/resources' },
                { text: 'Workflow', link: '/api/workflow' },
              ]
            },
            {
              text: 'Remote-Server Extensions',
              items: [
                { text: 'Auth (API Key + OAuth/CIMD)', link: '/api/auth' },
                { text: 'Batch Dispatch', link: '/api/batch' },
                { text: 'Elicitation', link: '/api/elicitation' },
                { text: 'Plugin Manifest', link: '/api/plugin-manifest' },
                { text: 'Rich Content (MCP Apps)', link: '/api/rich-content' },
                { text: 'DCC API Executor', link: '/api/dcc-api-executor' },
              ]
            }
          ]
        },
      }
    },
    zh: {
      label: '简体中文',
      lang: 'zh-CN',
      link: '/zh/',
      themeConfig: {
        nav: [
          { text: '指南', link: '/zh/guide/what-is-dcc-mcp-core' },
          { text: 'API', link: '/zh/api/models' },
          {
            text: 'v0.14.9',
            items: [
              { text: '更新日志', link: 'https://github.com/loonghao/dcc-mcp-core/blob/main/CHANGELOG.md' },
              { text: 'PyPI', link: 'https://pypi.org/project/dcc-mcp-core/' },
            ]
          }
        ],
        sidebar: {
          '/zh/guide/': [
            {
              text: '介绍',
              items: [
                { text: '什么是 DCC-MCP-Core？', link: '/zh/guide/what-is-dcc-mcp-core' },
                { text: '快速开始', link: '/zh/guide/getting-started' },
              ]
            },
            {
              text: 'MCP + Skills 系统',
              items: [
                { text: 'MCP + Skills 集成指南', link: '/zh/guide/mcp-skills-integration' },
                { text: 'Skills 技能包', link: '/zh/guide/skills' },
                { text: 'Skill 作用域与策略', link: '/zh/guide/skill-scopes-policies' },
                { text: '网关选举机制', link: '/zh/guide/gateway-election' },
                { text: 'Gateway', link: '/zh/guide/gateway' },
                { text: '远程服务器', link: '/zh/guide/remote-server' },
                { text: '生产环境部署', link: '/zh/guide/production-deployment' },
              ]
            },
            {
              text: '核心概念',
              items: [
                { text: 'Actions 动作', link: '/zh/guide/actions' },
                { text: '事件系统', link: '/zh/guide/events' },
                { text: 'MCP 协议', link: '/zh/guide/protocols' },
                { text: '命名 Actions 与 Tools', link: '/zh/guide/naming' },
                { text: '传输层', link: '/zh/guide/transport' },
                { text: 'Capabilities', link: '/zh/guide/capabilities' },
                { text: 'Prompts', link: '/zh/guide/prompts' },
              ]
            },
            {
              text: '进阶',
              items: [
                { text: '架构设计', link: '/zh/guide/architecture' },
                { text: '自定义 Skill', link: '/zh/guide/custom-actions' },
                { text: 'DCC 线程安全', link: '/zh/guide/dcc-thread-safety' },
                { text: '进程管理', link: '/zh/guide/process' },
                { text: '沙箱与安全', link: '/zh/guide/sandbox' },
                { text: '共享内存', link: '/zh/guide/shm' },
                { text: '遥测', link: '/zh/guide/telemetry' },
                { text: '画面捕获', link: '/zh/guide/capture' },
                { text: 'USD 桥接', link: '/zh/guide/usd' },
                { text: 'Artefacts', link: '/zh/guide/artefacts' },
                { text: 'Job Persistence', link: '/zh/guide/job-persistence' },
                { text: 'Scheduler', link: '/zh/guide/scheduler' },
                { text: 'Workflows', link: '/zh/guide/workflows' },
                { text: '常见问题', link: '/zh/guide/faq' },
              ]
            },
          ],
          '/zh/api/': [
            {
              text: 'API 参考',
              items: [
                { text: '数据模型', link: '/zh/api/models' },
                { text: 'Actions', link: '/zh/api/actions' },
                { text: '事件', link: '/zh/api/events' },
                { text: 'Skills', link: '/zh/api/skills' },
                { text: '协议', link: '/zh/api/protocols' },
                { text: '传输层', link: '/zh/api/transport' },
                { text: 'HTTP 服务器', link: '/zh/api/http' },
                { text: '进程管理', link: '/zh/api/process' },
                { text: '沙箱', link: '/zh/api/sandbox' },
                { text: '共享内存', link: '/zh/api/shm' },
                { text: '遥测', link: '/zh/api/telemetry' },
                { text: '画面捕获', link: '/zh/api/capture' },
                { text: 'USD', link: '/zh/api/usd' },
                { text: '工具函数', link: '/zh/api/utilities' },
                { text: '可观测性', link: '/zh/api/observability' },
                { text: 'Resources', link: '/zh/api/resources' },
                { text: 'Workflow', link: '/zh/api/workflow' },
              ]
            },
            {
              text: '远程服务器扩展',
              items: [
                { text: '认证 (API Key + OAuth/CIMD)', link: '/zh/api/auth' },
                { text: '批量分发', link: '/zh/api/batch' },
                { text: 'Elicitation 用户交互', link: '/zh/api/elicitation' },
                { text: '插件清单', link: '/zh/api/plugin-manifest' },
                { text: 'Rich Content (MCP Apps)', link: '/zh/api/rich-content' },
                { text: 'DCC API Executor', link: '/zh/api/dcc-api-executor' },
              ]
            }
          ]
        },
        outline: {
          label: '页面导航',
        },
        lastUpdated: {
          text: '最后更新于',
        },
        docFooter: {
          prev: '上一页',
          next: '下一页',
        },
      }
    }
  },

  themeConfig: {
    logo: '/logo.svg',
    socialLinks: [
      { icon: 'github', link: 'https://github.com/loonghao/dcc-mcp-core' }
    ],
    footer: {
      message: 'Released under the MIT License.',
      copyright: 'Copyright © 2025 Hal Long'
    },
    search: {
      provider: 'local'
    },
    editLink: {
      pattern: 'https://github.com/loonghao/dcc-mcp-core/edit/main/docs/:path'
    },
  },

  markdown: {
    lineNumbers: true,
  },
})
