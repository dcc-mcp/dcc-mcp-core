import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'DCC-MCP-Core',
  description: 'Foundational library for the DCC Model Context Protocol (MCP) ecosystem',
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
          { text: 'Guide', link: '/guide/getting-started' },
          { text: 'API', link: '/api/models' },
          {
            text: 'v0.12.0',
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
              text: 'Core Concepts',
              items: [
                { text: 'Actions & Registry', link: '/guide/actions' },
                { text: 'Event System', link: '/guide/events' },
                { text: 'Skills System', link: '/guide/skills' },
                { text: 'MCP Protocols', link: '/guide/protocols' },
                { text: 'Transport Layer', link: '/guide/transport' },
              ]
            },
            {
              text: 'Advanced',
              items: [
                { text: 'Custom Actions', link: '/guide/custom-actions' },
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
          { text: '指南', link: '/zh/guide/getting-started' },
          { text: 'API', link: '/zh/api/models' },
          {
            text: 'v0.12.0',
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
              text: '核心概念',
              items: [
                { text: 'Actions 动作', link: '/zh/guide/actions' },
                { text: '事件系统', link: '/zh/guide/events' },
                { text: 'Skills 技能包', link: '/zh/guide/skills' },
                { text: 'MCP 协议', link: '/zh/guide/protocols' },
                { text: '传输层', link: '/zh/guide/transport' },
              ]
            },
            {
              text: '进阶',
              items: [
                { text: '自定义 Action', link: '/zh/guide/custom-actions' },
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
