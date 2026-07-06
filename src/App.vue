<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  App.vue - 应用根组件

  功能说明:
  - 应用全局配置 (NaiveUI 黑白主题覆盖)
  - 布局组件加载
  - 初始化设置 (API 密钥加载)

  设计说明:
  - 全应用采用黑白编辑设计系统: 白底 #FFFFFF / 黑字 #000000 / 直角 / 1px 黑边框
  - 不再提供深色主题 (纯黑白系统只有一种形态)
  - NaiveUI 组件通过 themeOverrides 统一映射到黑白色板
-->

<script setup lang="ts">
// 导入 Vue 相关功能
import { onMounted } from "vue";

// 导入 NaiveUI 组件和类型
import { type GlobalThemeOverrides, NConfigProvider, NDialogProvider, NMessageProvider, NNotificationProvider } from "naive-ui";

// 导入 Store
import { useSettingsStore } from "@/stores/settings";

// 导入布局组件
import Layout from "@/components/Layout.vue";

// ============ 主题 ============

const fontSans = '"Inter Variable", "Inter", -apple-system, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif';

// 黑白编辑设计系统: 所有语义色一律折叠进黑-灰阶
const themeOverrides: GlobalThemeOverrides = {
  common: {
    fontFamily: fontSans,
    fontWeight: "400",
    fontWeightStrong: "600",

    // 缓动曲线: 统一使用 cubic-bezier(0.22, 1, 0.36, 1)
    cubicBezierEaseInOut: "cubic-bezier(0.22, 1, 0.36, 1)",
    cubicBezierEaseOut: "cubic-bezier(0.22, 1, 0.36, 1)",
    cubicBezierEaseIn: "cubic-bezier(0.22, 1, 0.36, 1)",

    // 直角
    borderRadius: "0",
    borderRadiusSmall: "0",

    // 主色 = 黑
    primaryColor: "#000000",
    primaryColorHover: "#444444",
    primaryColorPressed: "#000000",
    primaryColorSuppl: "#444444",

    // 语义色全部收敛为黑/灰
    infoColor: "#444444",
    infoColorHover: "#000000",
    infoColorPressed: "#000000",
    infoColorSuppl: "#888888",
    successColor: "#000000",
    successColorHover: "#444444",
    successColorPressed: "#000000",
    successColorSuppl: "#444444",
    warningColor: "#444444",
    warningColorHover: "#000000",
    warningColorPressed: "#000000",
    warningColorSuppl: "#888888",
    errorColor: "#000000",
    errorColorHover: "#444444",
    errorColorPressed: "#000000",
    errorColorSuppl: "#444444",

    // 文本
    textColorBase: "#000000",
    textColor1: "#000000",
    textColor2: "#444444",
    textColor3: "#888888",
    textColorDisabled: "#888888",
    placeholderColor: "#888888",
    placeholderColorDisabled: "#aaaaaa",
    iconColor: "#444444",
    iconColorHover: "#000000",
    iconColorPressed: "#000000",

    // 背景与表面
    baseColor: "#ffffff",
    bodyColor: "#ffffff",
    cardColor: "#ffffff",
    modalColor: "#ffffff",
    popoverColor: "#ffffff",
    tableColor: "#ffffff",
    tableHeaderColor: "#f5f5f5",
    inputColor: "#ffffff",
    actionColor: "#f5f5f5",
    hoverColor: "#f5f5f5",
    pressedColor: "#eeeeee",
    tableColorHover: "#f5f5f5",
    tableColorStriped: "#f5f5f5",
    buttonColor2: "#f5f5f5",
    buttonColor2Hover: "#eeeeee",
    buttonColor2Pressed: "#e8e8e8",
    tagColor: "#f5f5f5",
    avatarColor: "#f5f5f5",
    codeColor: "#f5f5f5",

    // 边框与分隔线
    borderColor: "#000000",
    dividerColor: "rgba(0, 0, 0, 0.4)",

    // 阴影: 纯黑低透明度
    boxShadow1: "0 4px 16px rgba(0, 0, 0, 0.06)",
    boxShadow2: "0 20px 60px rgba(0, 0, 0, 0.08)",
    boxShadow3: "0 24px 72px rgba(0, 0, 0, 0.12)",
  },
  Button: {
    // 悬浮反色: 黑底白字 → 白底黑字
    colorPrimary: "#000000",
    textColorPrimary: "#ffffff",
    colorHoverPrimary: "#ffffff",
    textColorHoverPrimary: "#000000",
    borderHoverPrimary: "1px solid #000000",
    colorPressedPrimary: "#000000",
    textColorPressedPrimary: "#ffffff",
    borderPressedPrimary: "1px solid #000000",
    colorFocusPrimary: "#000000",
    textColorFocusPrimary: "#ffffff",
    borderFocusPrimary: "1px solid #000000",

    // 默认按钮: 白底黑字 → 黑底白字
    color: "#ffffff",
    textColor: "#000000",
    border: "1px solid #000000",
    colorHover: "#000000",
    textColorHover: "#ffffff",
    borderHover: "1px solid #000000",
    colorPressed: "#000000",
    textColorPressed: "#ffffff",
    borderPressed: "1px solid #000000",
    textColorFocus: "#000000",
    borderFocus: "1px solid #000000",
  },
  Card: {
    borderColor: "rgba(0, 0, 0, 0.8)",
    borderRadius: "0",
  },
  Input: {
    borderHover: "1px solid #000000",
    borderFocus: "1px solid #000000",
    boxShadowFocus: "0 0 0 1px #000000",
  },
  Menu: {
    itemTextColor: "#444444",
    itemTextColorHover: "#000000",
    itemTextColorActive: "#000000",
    itemTextColorActiveHover: "#000000",
    itemIconColor: "#444444",
    itemIconColorHover: "#000000",
    itemIconColorActive: "#000000",
    itemIconColorActiveHover: "#000000",
    itemColorActive: "#f5f5f5",
    itemColorActiveHover: "#f5f5f5",
    borderRadius: "0",
  },
  Tag: {
    borderRadius: "0",
    border: "1px solid rgba(0, 0, 0, 0.6)",
  },
  Switch: {
    railColorActive: "#000000",
    boxShadowFocus: "0 0 0 1px rgba(0, 0, 0, 0.3)",
  },
  Tabs: {
    tabTextColorLine: "#888888",
    tabTextColorActiveLine: "#000000",
    tabTextColorHoverLine: "#000000",
    barColor: "#000000",
  },
  Dialog: {
    borderRadius: "0",
  },
  Modal: {
    borderRadius: "0",
  },
};

// ============ 响应式数据 ============

// 设置 Store
const settings = useSettingsStore();

// ============ 生命周期钩子 ============

// 组件挂载时的初始化
onMounted(async () => {
  // 从安全存储加载所有 API 密钥
  await settings.loadAllApiKeys();
  // 把当前的“关闭按钮行为”设置同步给后端（后端只在启动时给了默认值）
  await settings.syncCloseToTray();
  // 把当前的托盘唤起快捷键同步给后端注册（后端启动时只注册了默认值）
  await settings.syncShowHotkey();
});
</script>

<template>
  <n-config-provider
    :theme-overrides="themeOverrides"
    class="full-height"
  >
    <n-dialog-provider>
      <n-message-provider placement="bottom-left">
        <n-notification-provider placement="bottom-left">
          <Layout />
        </n-notification-provider>
      </n-message-provider>
    </n-dialog-provider>
  </n-config-provider>
</template>

<style lang="scss">
.full-height {
  height: 100%;
}
</style>
