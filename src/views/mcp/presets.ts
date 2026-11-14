/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// MCP 推荐能力 / 市场的静态数据与类型：从 MCPView.vue 原样迁出。

import type { MCPServer } from "@/stores/mcp";

/**
 * 推荐 MCP 服务预设
 * 均为社区广泛使用的官方实现 (modelcontextprotocol/servers 或对应厂商官方仓库)，
 * 通过 npx/uvx 在首次运行时自动下载，无需用户预先下载脚本
 */
export interface MCPPreset {
  id: string;
  name: string;
  description: string;
  serverType: "stdio" | "sse" | "http";
  command: string;
  args: string[];
  /** 若该服务需要用户修改参数 (如本地路径)，或有首次运行的前提条件，则提示文案 */
  needsConfig?: string;
  /** 市场里的分类标签，用于筛选；"推荐能力"卡片不展示分类，只有市场用 */
  category: string;
}

export const MCP_PRESETS: MCPPreset[] = [
  {
    id: "playwright",
    name: "浏览器自动化",
    description:
      "操控真实浏览器：打开网页、点击、填表、截图、读取页面结构等（微软官方 Playwright MCP）",
    serverType: "stdio",
    command: "npx",
    args: ["-y", "@playwright/mcp@latest"],
    needsConfig: "首次调用浏览器类工具时会自动下载 Chromium，请确保网络畅通，可能需要等待片刻",
    category: "网页与浏览器",
  },
  {
    id: "sequential-thinking",
    name: "顺序思维链",
    description: "帮助模型分步骤拆解复杂问题，结构化呈现推理过程",
    serverType: "stdio",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-sequential-thinking"],
    category: "效率与记忆",
  },
  {
    id: "time",
    name: "时间与时区",
    description: "获取当前时间、在不同时区之间转换",
    serverType: "stdio",
    command: "uvx",
    args: ["mcp-server-time"],
    category: "效率与记忆",
  },
  {
    id: "git",
    name: "Git 仓库工具",
    description: "读取、搜索本地 Git 仓库的提交、分支和文件",
    serverType: "stdio",
    command: "uvx",
    args: ["mcp-server-git", "--repository", "/path/to/git/repo"],
    needsConfig: "请将仓库路径改为你本地的 Git 仓库路径",
    category: "开发工具",
  },
  {
    id: "office-word",
    name: "Word 文档编辑",
    description:
      "创建、读取、编辑 .docx 文件：添加段落/标题/表格/图片，设置字体格式，合并文档，导出 PDF（基于 python-docx，2.1k Stars）",
    serverType: "stdio",
    command: "uvx",
    args: ["--from", "office-word-mcp-server", "word_mcp_server"],
    needsConfig:
      "需要已安装 uv（https://docs.astral.sh/uv/），首次运行会自动从 PyPI 拉取 office-word-mcp-server",
    category: "办公文档",
  },
  {
    id: "office-powerpoint",
    name: "PowerPoint 演示编辑",
    description:
      "创建、编辑 .pptx 文件：增删幻灯片、插入文字/图片/表格/图表，应用主题，提取文本（32 个工具，基于 python-pptx，1.8k Stars）",
    serverType: "stdio",
    command: "uvx",
    args: ["--from", "office-powerpoint-mcp-server", "ppt_mcp_server"],
    needsConfig:
      "需要已安装 uv（https://docs.astral.sh/uv/），首次运行会自动从 PyPI 拉取 office-powerpoint-mcp-server",
    category: "办公文档",
  },
  {
    id: "office-excel",
    name: "Excel 表格编辑",
    description:
      "读写 .xlsx 文件：读取/写入单元格、创建表格、复制 Sheet、设置格式（字体/填充/边框），基于 negokaz/excel-mcp-server（Go 实现，活跃维护）",
    serverType: "stdio",
    command: "npx",
    args: ["--yes", "@negokaz/excel-mcp-server"],
    category: "办公文档",
  },
];

/**
 * 市场专属条目：不进"推荐能力"（避免那张卡片过长），只在"MCP 市场"里可搜索到
 * 均为官方参考实现或活跃维护的社区方案，且都不需要天生就没有填写入口的 env 变量
 * （比如 GitHub/Slack 官方 MCP 需要 token 走 env，而现有表单没有 env 编辑项，暂不收录）
 */
export const MCP_MARKET_EXTRA: MCPPreset[] = [
  {
    id: "everything",
    name: "全功能测试服务",
    description:
      "官方参考实现，内置各类示例工具/资源/Prompt，用于验证 MCP 客户端的兼容程度（modelcontextprotocol/server-everything）",
    serverType: "stdio",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-everything"],
    category: "开发工具",
  },
  {
    id: "fetch",
    name: "网页抓取（官方版）",
    description: "抓取指定 URL 并转换为 Markdown 返回给模型，官方参考实现，可与内置的网页抓取搭配或替代使用",
    serverType: "stdio",
    command: "uvx",
    args: ["mcp-server-fetch"],
    category: "网页与浏览器",
  },
  {
    id: "postgres",
    name: "PostgreSQL 数据库",
    description: "只读查询 PostgreSQL 数据库结构与数据，官方参考实现",
    serverType: "stdio",
    command: "npx",
    args: [
      "-y",
      "@modelcontextprotocol/server-postgres",
      "postgresql://user:password@localhost:5432/dbname",
    ],
    needsConfig: "请将最后一行参数改为你的 PostgreSQL 连接字符串",
    category: "文件与数据",
  },
  {
    id: "sqlite",
    name: "SQLite 数据库",
    description: "读写本地 SQLite 数据库文件，支持查询与建表",
    serverType: "stdio",
    command: "uvx",
    args: ["mcp-server-sqlite", "--db-path", "/path/to/database.db"],
    needsConfig: "请将数据库路径改为你本地的 .db 文件路径",
    category: "文件与数据",
  },
  {
    id: "dingtalk",
    name: "钉钉机器人通知",
    description: "向钉钉群机器人发送文本/Markdown/链接消息，适合任务完成、会话结束等场景的主动通知",
    serverType: "stdio",
    command: "npx",
    args: ["-y", "claude-code-dingtalk-mcp"],
    needsConfig:
      "无需预先修改参数：添加后让 AI 调用 dingtalk_configure 工具，传入钉钉群机器人的 Webhook 地址（和可选的加签密钥）即可完成配置",
    category: "通知与协作",
  },
  {
    id: "feishu-lark",
    name: "飞书 / Lark 官方工具集",
    description: "飞书官方 OpenAPI MCP，覆盖消息、文档、多维表格、知识库、日历等接口，可读写飞书工作区内容",
    serverType: "stdio",
    command: "npx",
    args: ["-y", "@larksuiteoapi/lark-mcp", "mcp", "-a", "your_app_id", "-s", "your_app_secret"],
    needsConfig:
      "请将 -a 后的值改为飞书应用的 App ID，-s 后的值改为 App Secret（在飞书开放平台创建企业自建应用后获取）",
    category: "通知与协作",
  },
];

/** 市场目录 = 推荐能力里的预设 + 市场专属条目，二者共享同一套"填表单/查状态"逻辑 */
export const MCP_MARKET: MCPPreset[] = [...MCP_PRESETS, ...MCP_MARKET_EXTRA];

/** 市场分类筛选的选项列表，从目录里去重生成，不用另外手动维护一份 */
export const MCP_MARKET_CATEGORIES = Array.from(new Set(MCP_MARKET.map((entry) => entry.category)));

/**
 * 内置能力：应用原生实现，无需任何配置，随"启用 MCP"开关直接生效
 * 与 MCP_PRESETS 一起构成"推荐能力"模块，二者仅在是否需要用户配置上不同
 */
export interface BuiltinCapability {
  id: string;
  name: string;
  description: string;
}

export const BUILTIN_CAPABILITIES: BuiltinCapability[] = [
  {
    id: "web-search",
    name: "网络搜索",
    description:
      "通过 DuckDuckGo 搜索网页，获取实时信息。应用内置实现，无需安装 uv/Node.js 等任何依赖，随对话页的\"启用 MCP\"开关一起生效；Agent Team 里每个 Agent 也默认可用，不需要额外勾选",
  },
  {
    id: "web-fetch",
    name: "网页内容抓取",
    description:
      "抓取网页并提取正文文本。应用内置实现，无需安装任何依赖，随对话页的\"启用 MCP\"开关一起生效；Agent Team 里每个 Agent 也默认可用，不需要额外勾选",
  },
  {
    id: "terminal",
    name: "终端执行",
    description:
      "在当前聊天的工作目录中执行命令。每次执行都需要你确认；可在下方选择默认使用的终端。",
  },
  {
    id: "ask-user",
    name: "向用户提问",
    description:
      "让模型在需要澄清信息时向你提问，并在收到回答后继续完成当前回复。",
  },
];

/** "推荐能力"列表里的一项：内置能力（零配置）或社区预设（需要用户点击后填表保存） */
export type RecommendedItem =
  | { kind: "builtin"; capability: BuiltinCapability }
  | { kind: "preset"; preset: MCPPreset };

/**
 * 判断某个预设是否已经被配置成一个已保存的服务器
 * 用名称匹配（预设应用后表单名称默认沿用 preset.name，用户当然可以改名，
 * 改名后会被判定为"未配置"，这是可接受的简单启发式）
 */
export const isPresetConfigured = (preset: MCPPreset, servers: MCPServer[]) =>
  servers.some((server) => server.name === preset.name);
