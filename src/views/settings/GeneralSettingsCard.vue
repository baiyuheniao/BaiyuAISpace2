<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  GeneralSettingsCard.vue - 通用设置卡片（外观 / 聊天显示 / 对话与工具 / 系统行为 / 安全与数据）
  （从 SettingsView.vue 拆出，逻辑与模板原样搬迁。）
-->

<script setup lang="ts">
import { ref, onBeforeUnmount } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { NCard, NSelect, NInput, NInputNumber, NButton, NSpace, NSwitch, NTag, NPopconfirm, NIcon, NText } from "naive-ui";
import { useMessage } from "@/composables/useNotify";
import {
  useSettingsStore,
  type ErrorSoundLevel,
  type StartupWindowMode,
  type FontSize,
  type ThemeMode,
  type ColorTone,
} from "@/stores/settings";
import {
  SettingsOutline,
} from "@vicons/ionicons5";
import { isModifierOnly, acceleratorFromEvent } from "@/utils/hotkey";

const settings = useSettingsStore();
const message = useMessage();

const errorSoundLevelOptions: Array<{
  label: string;
  value: ErrorSoundLevel;
}> = [
  { label: "仅严重错误", value: "critical" },
  { label: "所有错误", value: "all" },
  { label: "关闭声音", value: "off" },
];

const handleErrorSoundLevelChange = (value: string) => {
  if (value === "off" || value === "critical" || value === "all") {
    settings.errorSoundLevel = value;
  }
};

// 选择本地图片作为头像：经后端读成 data URL 后写入设置，
// 这样前端直接渲染，不需要 asset 协议或 fs 权限。
const pickAvatar = async (target: "ai" | "user") => {
  const selected = await open({
    multiple: false,
    filters: [
      { name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp", "svg", "ico", "bmp"] },
    ],
  });
  if (typeof selected !== "string") return;
  try {
    const dataUrl = await invoke<string>("read_image_as_data_url", { path: selected });
    if (target === "ai") settings.aiAvatar = dataUrl;
    else settings.userAvatar = dataUrl;
  } catch (error) {
    message.error(`读取头像失败：${error}`);
  }
};

const fontSizeOptions: Array<{ label: string; value: FontSize }> = [
  { label: "小", value: "small" },
  { label: "标准", value: "medium" },
  { label: "大", value: "large" },
];

const themeModeOptions: Array<{ label: string; value: ThemeMode }> = [
  { label: "跟随系统", value: "system" },
  { label: "浅色", value: "light" },
  { label: "深色", value: "dark" },
];

const handleThemeModeChange = (value: string) => {
  if (value === "light" || value === "dark" || value === "system") {
    settings.setThemeMode(value);
  }
};

const colorToneOptions: Array<{ label: string; value: ColorTone }> = [
  { label: "高对比度", value: "contrast" },
  { label: "正常色调", value: "normal" },
];

const handleColorToneChange = (value: string) => {
  if (value === "contrast" || value === "normal") {
    settings.setColorTone(value);
  }
};

const handleFontSizeChange = (value: string) => {
  if (value === "small" || value === "medium" || value === "large") {
    settings.setFontSize(value);
  }
};

const startupWindowModeOptions: Array<{ label: string; value: StartupWindowMode }> = [
  { label: "普通窗口", value: "window" },
  { label: "全屏", value: "fullscreen" },
];

const handleStartupWindowModeChange = async (value: string) => {
  if (value !== "window" && value !== "fullscreen") return;
  try {
    await settings.setStartupWindowMode(value);
    message.success(`已设置：下次启动时${value === "fullscreen" ? "全屏打开" : "以普通窗口打开"}`);
  } catch (error) {
    message.error(`启动窗口模式设置失败：${error}`);
  }
};

// ============ 通用设置 ============

/**
 * 切换“关闭窗口时最小化到系统托盘”开关
 * 同时把设置同步给 Rust 后端（窗口关闭事件在后端拦截）
 */
const handleCloseToTrayChange = async (enabled: boolean) => {
  await settings.setCloseToTray(enabled);
  message.success(enabled ? "已开启：关闭窗口将最小化到托盘" : "已关闭：关闭窗口将直接退出程序");
};

// ============ 危险操作：清空数据库 ============

/** 清空数据库中：会话、消息、MCP 服务器配置、Skill。知识库 / 协作团队 /
 * 定时任务是各自独立的 SQLite 文件，不受影响。 */
const clearingDatabase = ref(false);

const handleClearDatabase = async () => {
  clearingDatabase.value = true;
  try {
    await invoke("clear_database_cmd");
    message.success("数据库已清空：会话、消息、MCP 服务器配置、Skill 均已删除");
  } catch (error) {
    message.error(`清空数据库失败：${error}`);
  } finally {
    clearingDatabase.value = false;
  }
};

// ============ 快捷键录制（托盘唤起 / 新建会话 / 切换全屏） ============

/** 当前正在录制哪个快捷键；null 表示未在录制 */
const recordingHotkeyTarget = ref<"show" | "newSession" | "fullscreen" | null>(null);

let hotkeyRecordListener: ((e: KeyboardEvent) => void) | null = null;

const stopRecordingHotkey = () => {
  if (hotkeyRecordListener) {
    window.removeEventListener("keydown", hotkeyRecordListener, true);
    hotkeyRecordListener = null;
  }
  recordingHotkeyTarget.value = null;
};

const startRecordingHotkey = (target: "show" | "newSession" | "fullscreen") => {
  if (recordingHotkeyTarget.value) return;
  recordingHotkeyTarget.value = target;

  hotkeyRecordListener = async (e: KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();

    if (e.key === "Escape") {
      stopRecordingHotkey();
      return;
    }

    // 纯修饰键还没构成完整组合，继续等待主键
    if (isModifierOnly(e)) return;

    const accelerator = acceleratorFromEvent(e);
    if (!accelerator) {
      message.warning("请至少搭配一个修饰键（Ctrl / Alt / Shift）或使用功能键（F1~F24），避免和普通按键冲突");
      return;
    }

    stopRecordingHotkey();

    if (target === "show") {
      try {
        await settings.setShowHotkey(accelerator);
        message.success(`唤起快捷键已设置为 ${accelerator}`);
      } catch (error) {
        message.error(String(error));
      }
    } else if (target === "newSession") {
      settings.setNewSessionHotkey(accelerator);
      message.success(`新建会话快捷键已设置为 ${accelerator}`);
    } else {
      settings.setFullscreenHotkey(accelerator);
      message.success(`切换全屏快捷键已设置为 ${accelerator}`);
    }
  };

  window.addEventListener("keydown", hotkeyRecordListener, true);
};

onBeforeUnmount(() => {
  stopRecordingHotkey();
});
</script>

<template>
  <!-- 通用设置卡片 -->
  <n-card
    class="settings-card"
    :bordered="false"
  >
    <template #header>
      <div class="card-header">
        <n-icon
          :size="20"
          depth="3"
        >
          <SettingsOutline />
        </n-icon>
        <span>通用设置</span>
      </div>
    </template>

    <div class="settings-group-title">外观</div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">显示模式</span>
        <n-text depth="3" style="font-size: 12px;">
          深色模式沿用黑白编辑风格；选择“跟随系统”会在系统外观变化时自动切换。
        </n-text>
      </div>
      <n-select
        :value="settings.themeMode"
        :options="themeModeOptions"
        placeholder="请选择显示模式"
        style="width: 180px;"
        @update:value="handleThemeModeChange"
      />
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">色调</span>
        <n-text depth="3" style="font-size: 12px;">
          高对比度为纯黑纯白（#000000 + #FFFFFF）；正常色调整体仍是黑白灰，但明暗反差更柔和，对散光和敏感眼更友好。浅色与深色模式会分别适配。
        </n-text>
      </div>
      <n-select
        :value="settings.colorTone"
        :options="colorToneOptions"
        placeholder="请选择色调"
        style="width: 180px;"
        @update:value="handleColorToneChange"
      />
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">字体大小</span>
        <n-text depth="3" style="font-size: 12px;">
          调整软件的基础字号；修改后会立即应用，并在下次启动时保留。
        </n-text>
      </div>
      <n-select
        :value="settings.fontSize"
        :options="fontSizeOptions"
        placeholder="请选择字体大小"
        style="width: 180px;"
        @update:value="handleFontSizeChange"
      />
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">显示侧边栏内部框线</span>
        <n-text
          depth="3"
          style="font-size: 12px;"
        >
          开启后显示 Logo、新建对话、导航和底部区域的装饰框线；关闭时使用更简洁的无框线侧边栏。
        </n-text>
      </div>
      <n-switch v-model:value="settings.sidebarInternalBordersEnabled" />
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">消息内容宽度</span>
        <n-text depth="3" style="font-size: 12px;">聊天页消息列表的最大内容宽度（像素）。</n-text>
      </div>
      <n-input-number
        v-model:value="settings.chatContentWidth"
        :min="480"
        :max="2000"
        style="width: 130px"
      >
        <template #suffix>px</template>
      </n-input-number>
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">输入框聚焦时上浮</span>
        <n-text depth="3" style="font-size: 12px;">输入框获得焦点时轻微上浮并显示阴影；关闭后保持静止。</n-text>
      </div>
      <n-switch v-model:value="settings.inputFocusLiftEnabled" />
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">消息体显示边框</span>
        <n-text depth="3" style="font-size: 12px;">每条消息的正文卡片是否显示细边框。</n-text>
      </div>
      <n-switch v-model:value="settings.messageBorderEnabled" />
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">默认启用 MCP 工具</span>
        <n-text depth="3" style="font-size: 12px;">
          开启后，每次启动软件时聊天页侧边栏的「MCP 工具」默认为开；聊天页内仍可临时切换，不影响此默认值。
        </n-text>
      </div>
      <n-switch v-model:value="settings.chatMcpDefaultEnabled" />
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">默认允许模型自主调用 Skill</span>
        <n-text depth="3" style="font-size: 12px;">
          开启后，每次启动软件时聊天页侧边栏的「模型可自主调用 Skill」默认为开；聊天页内仍可临时切换，不影响此默认值。
        </n-text>
      </div>
      <n-switch v-model:value="settings.chatSkillAutonomyDefaultEnabled" />
    </div>

    <div class="settings-group-title">聊天显示</div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">AI 头像</span>
        <n-text depth="3" style="font-size: 12px;">填 http(s) 链接，或点右侧按钮选本地图片；留空使用默认 Logo。</n-text>
      </div>
      <n-space
        align="center"
        :size="8"
      >
        <n-input
          v-model:value="settings.aiAvatar"
          placeholder="图片 URL 或 data URL"
          style="width: 300px"
        />
        <n-button
          size="small"
          @click="pickAvatar('ai')"
        >
          选择本地图片
        </n-button>
      </n-space>
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">用户头像</span>
        <n-text depth="3" style="font-size: 12px;">填 http(s) 链接，或点右侧按钮选本地图片；留空使用默认 Logo。</n-text>
      </div>
      <n-space
        align="center"
        :size="8"
      >
        <n-input
          v-model:value="settings.userAvatar"
          placeholder="图片 URL 或 data URL"
          style="width: 300px"
        />
        <n-button
          size="small"
          @click="pickAvatar('user')"
        >
          选择本地图片
        </n-button>
      </n-space>
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">AI 显示名</span>
        <n-text depth="3" style="font-size: 12px;">聊天中 AI 消息的作者名。</n-text>
      </div>
      <n-input
        v-model:value="settings.aiName"
        placeholder="BAI"
        style="width: 180px"
      />
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">用户显示名</span>
        <n-text depth="3" style="font-size: 12px;">聊天中你自己消息的作者名。</n-text>
      </div>
      <n-input
        v-model:value="settings.userName"
        placeholder="用户"
        style="width: 180px"
      />
    </div>

    <div class="settings-group-title">对话与工具</div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">内置文件工具限制</span>
        <n-text depth="3" style="font-size: 12px;">控制单次读取、目录列表和搜索的返回量；超出上限时工具会明确提示。</n-text>
      </div>
      <n-space>
        <n-input-number v-model:value="settings.fileReadLimitMb" placeholder="读取上限" :min="1" :max="16" style="width: 120px"><template #suffix>MB</template></n-input-number>
        <n-input-number v-model:value="settings.fileListLimit" placeholder="列表条数" :min="10" :max="5000" style="width: 120px"><template #suffix>条</template></n-input-number>
        <n-input-number v-model:value="settings.fileSearchLimit" placeholder="搜索条数" :min="10" :max="1000" style="width: 120px"><template #suffix>条</template></n-input-number>
      </n-space>
    </div>

    <div class="general-setting-item general-setting-item--stack">
      <div class="general-setting-text">
        <span class="general-setting-label">全局 System Prompt</span>
        <n-text
          depth="3"
          style="font-size: 12px;"
        >
          对之后发送的每条新消息生效，会自动附加到对话的系统消息中，用于统一设定模型的身份、语气或回答规范；留空则不附加。已发送的历史消息不受影响。
        </n-text>
      </div>
      <n-input
        v-model:value="settings.systemPrompt"
        type="textarea"
        placeholder="例如：你是一个简洁、专业的助手，优先给出可执行的建议，避免空泛的客套话。"
        :autosize="{ minRows: 3, maxRows: 8 }"
      />
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">请求失败自动重试</span>
        <n-text
          depth="3"
          style="font-size: 12px;"
        >
          当服务商返回限流或过载类错误（如 429、模型引擎过载）时，自动按此配置重试。重试次数设为 0 即关闭自动重试。
        </n-text>
      </div>
      <n-space
        align="center"
        :size="12"
      >
        <n-input-number
          v-model:value="settings.retryCount"
          placeholder="重试次数"
          :min="0"
          :max="10"
          style="width: 140px;"
        >
          <template #suffix>
            次
          </template>
        </n-input-number>
        <n-input-number
          v-model:value="settings.retryIntervalSecs"
          placeholder="重试间隔"
          :min="1"
          :max="60"
          style="width: 140px;"
        >
          <template #suffix>
            秒
          </template>
        </n-input-number>
      </n-space>
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">最大工具调用轮数</span>
        <n-text depth="3" style="font-size: 12px;">
          普通 Chat 在一次回答中最多连续调用工具的轮数。达到上限后，模型会基于已获得的结果收尾并说明未完成项。
        </n-text>
      </div>
      <n-input-number
        v-model:value="settings.maxToolRounds"
        placeholder="请输入 1–100"
        :min="1"
        :max="100"
        style="width: 140px;"
      >
        <template #suffix>轮</template>
      </n-input-number>

    <div class="settings-group-title">系统行为</div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">关闭窗口时最小化到系统托盘</span>
        <n-text
          depth="3"
          style="font-size: 12px;"
        >
          开启后，点击窗口右上角的关闭按钮只会隐藏窗口，程序继续在系统托盘运行；需从托盘图标菜单选择“退出程序”才会真正结束。关闭后，点击关闭按钮将直接退出程序。
        </n-text>
      </div>
      <n-switch
        :value="settings.closeToTray"
        @update:value="handleCloseToTrayChange"
      />
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">默认启动窗口模式</span>
        <n-text
          depth="3"
          style="font-size: 12px;"
        >
          选择首次启动或重新选择模式后的打开方式。软件会自动记住上次的窗口位置、尺寸、最大化和全屏状态；之后启动时优先恢复该状态。
        </n-text>
      </div>
      <n-select
        :value="settings.startupWindowMode"
        :options="startupWindowModeOptions"
        placeholder="请选择默认启动窗口模式"
        style="width: 180px;"
        @update:value="handleStartupWindowModeChange"
      />
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">报错声音提醒</span>
        <n-text
          depth="3"
          style="font-size: 12px;"
        >
          报错弹窗始终会显示。选择“仅严重错误”时，只有后台 Agent 中断、本地保存失败等需要立即介入的问题会响；选择“所有错误”后，普通操作失败也会响。
        </n-text>
      </div>
      <n-select
        :value="settings.errorSoundLevel"
        :options="errorSoundLevelOptions"
        placeholder="请选择报错声音等级"
        style="width: 180px;"
        @update:value="handleErrorSoundLevelChange"
      />
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">从托盘唤起窗口的快捷键</span>
        <n-text
          depth="3"
          style="font-size: 12px;"
        >
          在任意界面按下该组合键，即可把最小化到托盘的窗口唤回桌面。点击右侧按钮后按下新的组合键即可修改，按 Esc 取消录制。
        </n-text>
      </div>
      <n-space
        align="center"
        :size="12"
      >
        <n-tag size="medium">
          {{ settings.showHotkey }}
        </n-tag>
        <n-button
          size="small"
          :type="recordingHotkeyTarget === 'show' ? 'warning' : 'default'"
          @click="startRecordingHotkey('show')"
        >
          {{ recordingHotkeyTarget === 'show' ? '请按下组合键…' : '修改快捷键' }}
        </n-button>
      </n-space>
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">新建会话的快捷键</span>
        <n-text
          depth="3"
          style="font-size: 12px;"
        >
          在应用窗口获得焦点时按下该组合键，即可快速开始一个新对话。点击右侧按钮后按下新的组合键即可修改，按 Esc 取消录制。
        </n-text>
      </div>
      <n-space
        align="center"
        :size="12"
      >
        <n-tag size="medium">
          {{ settings.newSessionHotkey }}
        </n-tag>
        <n-button
          size="small"
          :type="recordingHotkeyTarget === 'newSession' ? 'warning' : 'default'"
          @click="startRecordingHotkey('newSession')"
        >
          {{ recordingHotkeyTarget === 'newSession' ? '请按下组合键…' : '修改快捷键' }}
        </n-button>
      </n-space>
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">切换全屏的快捷键</span>
        <n-text
          depth="3"
          style="font-size: 12px;"
        >
          在应用窗口获得焦点时按下该快捷键，即可切换窗口全屏 / 还原。点击右侧按钮后按下新的按键即可修改，按 Esc 取消录制。
        </n-text>
      </div>
      <n-space
        align="center"
        :size="12"
      >
        <n-tag size="medium">
          {{ settings.fullscreenHotkey }}
        </n-tag>
        <n-button
          size="small"
          :type="recordingHotkeyTarget === 'fullscreen' ? 'warning' : 'default'"
          @click="startRecordingHotkey('fullscreen')"
        >
          {{ recordingHotkeyTarget === 'fullscreen' ? '请按下按键…' : '修改快捷键' }}
        </n-button>
      </n-space>
    </div>

    <div class="settings-group-title settings-group-title--danger">安全与数据</div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">允许抓取本机与内网地址</span>
        <n-text depth="3" style="font-size: 12px;">
          默认关闭，用于防止网页抓取被诱导访问本机或局域网服务。仅在你需要调试自己信任的开发服务时开启。
        </n-text>
      </div>
      <n-switch v-model:value="settings.allowLocalNetworkFetch" />
    </div>

    <div class="general-setting-item">
      <div class="general-setting-text">
        <span class="general-setting-label">清空数据库</span>
        <n-text
          depth="3"
          style="font-size: 12px;"
        >
          永久删除全部会话、聊天记录、MCP 服务器配置、Skill，操作不可撤销。知识库、协作团队、定时任务数据不受影响。建议清空前自行备份重要对话。
        </n-text>
      </div>
      <n-popconfirm
        positive-text="确认清空"
        negative-text="取消"
        @positive-click="handleClearDatabase"
      >
        <template #trigger>
          <n-button
            size="small"
            type="error"
            :loading="clearingDatabase"
          >
            清空数据库
          </n-button>
        </template>
        此操作将永久删除全部会话、聊天记录、MCP 服务器配置、Skill，且无法恢复，确定继续？
      </n-popconfirm>
    </div>
    </div>
  </n-card>
</template>

<style scoped lang="scss">
/* 通用设置分组标题 */
.settings-group-title {
  margin-top: 28px;
  margin-bottom: 12px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--color-line);
  font-size: 0.75rem;
  letter-spacing: 0.15em;
  color: $ink-soft;
}

.settings-group-title:first-child {
  margin-top: 0;
}

/* 危险/安全组：黑白单色下用加重分隔线 + 更深字色表达，不用彩色 */
.settings-group-title--danger {
  border-bottom-width: 2px;
  color: $ink;
}

/* 通用设置项 */
.general-setting-item {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 24px;
}

.general-setting-item + .general-setting-item {
  margin-top: 20px;
  padding-top: 20px;
  border-top: 1px solid var(--color-line-faint);
}

.general-setting-item--stack {
  flex-direction: column;
  align-items: stretch;
}

.general-setting-text {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.general-setting-label {
  font-weight: 600;
}
</style>
