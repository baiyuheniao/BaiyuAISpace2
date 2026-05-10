/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/**
 * 应用入口文件
 * 
 * 功能说明:
 * - 创建 Vue 应用实例
 * - 配置 Pinia 状态管理 (含持久化插件)
 * - 注册 Vue Router
 * - 挂载应用到 DOM
 */

import { createApp } from "vue";
import { createPinia } from "pinia";
import piniaPluginPersistedstate from "pinia-plugin-persistedstate";
import router from "./router";
import App from "./App.vue";

// 创建 Pinia 实例
const pinia = createPinia();
// 使用持久化插件 (将状态保存到 localStorage)
pinia.use(piniaPluginPersistedstate);

// 创建 Vue 应用实例
const app = createApp(App);

// 注册 Pinia 状态管理
app.use(pinia);

// 注册路由
app.use(router);

// 挂载应用到 #app 元素
app.mount("#app");
