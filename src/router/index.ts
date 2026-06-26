/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/**
 * 路由配置 - 定义应用的所有路由
 * 
 * 路由说明:
 * - 使用 hash 模式 (createWebHashHistory) 适用于 Tauri 应用
 * - 懒加载视图组件以优化首屏加载速度
 * 
 * 路由列表:
 * - /: 聊天页面 (ChatView)
 * - /knowledge-base: 知识库页面 (KnowledgeBaseView)
 * - /mcp: MCP 管理页面 (MCPView)
 * - /skills: Skill 管理页面 (SkillsView)
 * - /local-deploy: 本地部署页面 (LocalDeployView)
 * - /history: 历史记录页面 (HistoryView)
 * - /settings: 设置页面 (SettingsView)
 */

import { createRouter, createWebHashHistory } from "vue-router";
import type { RouteRecordRaw } from "vue-router";

// 路由配置数组
const routes: RouteRecordRaw[] = [
  {
    path: "/",
    name: "Chat",
    component: () => import("@/views/ChatView.vue"),
  },
  {
    path: "/knowledge-base",
    name: "KnowledgeBase",
    component: () => import("@/views/KnowledgeBaseView.vue"),
  },
  {
    path: "/mcp",
    name: "MCP",
    component: () => import("@/views/MCPView.vue"),
  },
  {
    path: "/skills",
    name: "Skills",
    component: () => import("@/views/SkillsView.vue"),
  },
  {
    path: "/local-deploy",
    name: "LocalDeploy",
    component: () => import("@/views/LocalDeployView.vue"),
  },
  {
    path: "/history",
    name: "History",
    component: () => import("@/views/HistoryView.vue"),
  },
  {
    path: "/settings",
    name: "Settings",
    component: () => import("@/views/SettingsView.vue"),
  },
  {
    path: "/agent-team",
    name: "AgentTeam",
    component: () => import("@/views/AgentTeamView.vue"),
  },
  {
    path: "/scheduler",
    name: "Scheduler",
    component: () => import("@/views/SchedulerView.vue"),
  },
];

// 创建路由器实例
const router = createRouter({
  history: createWebHashHistory(),
  routes,
});

// 导出路由器供主应用使用
export default router;
