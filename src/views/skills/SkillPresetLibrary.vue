<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  SkillPresetLibrary.vue - Skill 预设库卡片

  开发者 / 商务 / 通用三类预设，一键导入为用户自己的 Skill。预设数据见 ./presets.ts。
-->

<script setup lang="ts">
import { ref } from "vue";
import {
  NCard,
  NButton,
  NText,
  NTabs,
  NTabPane,
  NSpace,
  NIcon,
} from "naive-ui";
import { useMessage } from "@/composables/useNotify";
import {
  DownloadOutline,
  CodeSlashOutline,
  BriefcaseOutline,
  SparklesOutline,
} from "@vicons/ionicons5";
import { useSkillsStore } from "@/stores/skills";
import {
  DEV_PRESETS,
  BIZ_PRESETS,
  GENERAL_PRESETS,
  type SkillPreset,
} from "./presets";

const skillsStore = useSkillsStore();
const message = useMessage();

const categories = [
  { name: "dev", label: "开发者", icon: CodeSlashOutline, presets: DEV_PRESETS },
  { name: "biz", label: "商务", icon: BriefcaseOutline, presets: BIZ_PRESETS },
  { name: "general", label: "通用", icon: SparklesOutline, presets: GENERAL_PRESETS },
];

const importingPreset = ref<string | null>(null);

const isPresetImported = (presetName: string): boolean =>
  skillsStore.skills.some((s) => s.name === presetName);

const handleImportPreset = async (preset: SkillPreset) => {
  if (isPresetImported(preset.name)) return;
  importingPreset.value = preset.name;
  await skillsStore.saveSkill({
    id: "",
    name: preset.name,
    description: preset.description,
    instructions: preset.instructions,
    boundMcpServerIds: [],
    enabled: true,
    resourceFiles: [],
  });
  importingPreset.value = null;
  message.success(`「${preset.name}」已导入`);
};
</script>

<template>
  <n-card
    class="settings-card"
    :bordered="false"
  >
    <template #header>
      <div class="card-header">
        <n-icon :size="20" depth="3"><DownloadOutline /></n-icon>
        <span>预设库</span>
      </div>
    </template>

    <n-tabs type="segment" animated>
      <n-tab-pane
        v-for="cat in categories"
        :key="cat.name"
        :name="cat.name"
        :tab="cat.label"
      >
        <template #tab>
          <n-space size="small" align="center">
            <n-icon><component :is="cat.icon" /></n-icon>
            {{ cat.label }}
          </n-space>
        </template>
        <div class="preset-grid">
          <n-card
            v-for="preset in cat.presets"
            :key="preset.name"
            size="small"
            :bordered="true"
            class="preset-card"
          >
            <div class="preset-name">{{ preset.name }}</div>
            <div class="preset-desc">{{ preset.description }}</div>
            <template #footer>
              <n-button
                size="small"
                :type="isPresetImported(preset.name) ? 'default' : 'primary'"
                :disabled="isPresetImported(preset.name)"
                :loading="importingPreset === preset.name"
                @click="handleImportPreset(preset)"
              >
                {{ isPresetImported(preset.name) ? "已导入" : "导入" }}
              </n-button>
            </template>
          </n-card>
        </div>
      </n-tab-pane>
    </n-tabs>

    <template #footer>
      <n-text depth="3" style="font-size: 12px">
        导入后可在「已配置的 Skill」里编辑指令内容或绑定 MCP 工具
      </n-text>
    </template>
  </n-card>
</template>

<style scoped lang="scss">
/* 预设卡片网格 —— 原来是 n-grid 固定两列，宽屏下永远只有两列、两侧一堆空白。
   换成 CSS Grid 的 auto-fill，卡片按可用宽度自动增加列数 */
.preset-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
  gap: 12px;
  margin-top: 12px;
}

.preset-card {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.preset-name {
  font-family: $font-serif;
  font-size: 14px;
  font-weight: 700;
  margin-bottom: 6px;
  color: $ink;
}

.preset-desc {
  font-size: 12px;
  color: $ink-faint;
  line-height: $leading-body;
}
</style>
