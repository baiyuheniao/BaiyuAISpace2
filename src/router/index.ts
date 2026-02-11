/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { createRouter, createWebHashHistory } from "vue-router";
import type { RouteRecordRaw } from "vue-router";

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
    path: "/history",
    name: "History",
    component: () => import("@/views/HistoryView.vue"),
  },
  {
    path: "/settings",
    name: "Settings",
    component: () => import("@/views/SettingsView.vue"),
  },
];

const router = createRouter({
  history: createWebHashHistory(),
  routes,
});

export default router;
