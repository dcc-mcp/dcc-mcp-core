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
            text: 'v0.14.1',
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
              text: 'v0.14.1',
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
