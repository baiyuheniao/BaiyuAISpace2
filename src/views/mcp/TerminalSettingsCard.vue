<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  TerminalSettingsCard.vue - 内置终端设置卡片

  选择"终端执行"内置工具默认使用的 shell，存在 localStorage（chat.ts 发送消息时读取同一个 key）。
-->

<script setup lang="ts">
import { ref, onMounted } from "vue";
import { NCard, NFormItem, NSelect, NText, NIcon } from "naive-ui";
import { Terminal } from "@vicons/ionicons5";

const terminalShell = ref<"powershell" | "cmd" | "git-bash">("powershell");
const terminalShellOptions = [
  { label: "PowerShell", value: "powershell" },
  { label: "CMD", value: "cmd" },
  { label: "Git Bash", value: "git-bash" },
];

const saveTerminalShell = (value: "powershell" | "cmd" | "git-bash") => {
  terminalShell.value = value;
  localStorage.setItem("baiyu.mcp.terminal-shell", value);
};

onMounted(() => {
  const saved = localStorage.getItem("baiyu.mcp.terminal-shell");
  if (saved === "powershell" || saved === "cmd" || saved === "git-bash") terminalShell.value = saved;
});
</script>

<template>
  <n-card
    class="settings-card"
    :bordered="false"
  >
    <template #header>
      <div class="card-header">
        <n-icon :size="20" depth="3"><Terminal /></n-icon>
        <span>内置终端</span>
      </div>
    </template>
    <n-form-item label="默认终端">
      <n-select
        :value="terminalShell"
        :options="terminalShellOptions"
        placeholder="选择默认终端"
        @update:value="saveTerminalShell"
      />
      <template #feedback>
        <n-text depth="3" style="font-size: 12px">
          终端只会在当前聊天已设置的工作目录中运行；每条命令执行前都会再次向你确认。
        </n-text>
      </template>
    </n-form-item>
  </n-card>
</template>
