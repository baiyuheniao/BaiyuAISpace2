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
 * - 注册 v-reveal 滚动揭示指令 (IntersectionObserver)
 * - 挂载应用到 DOM
 */

import { createApp, type Directive } from "vue";
import { createPinia } from "pinia";
import piniaPluginPersistedstate from "pinia-plugin-persistedstate";
import router from "./router";
import App from "./App.vue";

// 字体：中文标题 Noto Serif SC（衬线），正文 Inter（无衬线）
import "@fontsource-variable/inter";
import "@fontsource-variable/noto-serif-sc";

// 全局样式（黑白编辑设计系统）
import "./styles/global.scss";

/**
 * v-reveal 滚动揭示指令
 *
 * 元素进入视口 15% 时添加 .is-visible，触发入场动画
 * (opacity 0→1, translateY 40px→0, scale 0.95→1)。
 * 支持 v-reveal="120" 传入延迟毫秒数实现级联入场。
 */
const revealObserver = new IntersectionObserver(
  (entries) => {
    for (const entry of entries) {
      if (entry.isIntersecting) {
        const el = entry.target as HTMLElement;
        const delay = Number(el.dataset.revealDelay || 0);
        if (delay > 0) {
          el.style.transitionDelay = `${delay}ms`;
        }
        el.classList.add("is-visible");
        revealObserver.unobserve(el);
      }
    }
  },
  { threshold: 0.15 },
);

const vReveal: Directive<HTMLElement, number | undefined> = {
  mounted(el, binding) {
    el.classList.add("reveal");
    if (binding.value) {
      el.dataset.revealDelay = String(binding.value);
    }
    revealObserver.observe(el);
  },
  unmounted(el) {
    revealObserver.unobserve(el);
  },
};

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

// 注册滚动揭示指令
app.directive("reveal", vReveal);

// 挂载应用到 #app 元素
app.mount("#app");
