<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  SkillsView.vue - Skill (技能) 管理视图

  页面外壳：标题 + 三个子组件（列表 / 预设库 / 编辑弹窗）。
  - SkillList：已配置的 Skill，启用/禁用/删除
  - SkillPresetLibrary：预设库一键导入
  - SkillEditModal：新建/编辑，含绑定 MCP 服务器与资源文件管理
-->

<script setup lang="ts">
import { ref, onMounted } from "vue";
import { NLayout, NLayoutContent } from "naive-ui";
import { useSkillsStore, type Skill } from "@/stores/skills";
import { useMCPStore } from "@/stores/mcp";
import SkillList from "./skills/SkillList.vue";
import SkillPresetLibrary from "./skills/SkillPresetLibrary.vue";
import SkillEditModal from "./skills/SkillEditModal.vue";

const skillsStore = useSkillsStore();
const mcp = useMCPStore();

const editModal = ref<InstanceType<typeof SkillEditModal> | null>(null);

const openCreate = () => editModal.value?.open();
const openEdit = (skill: Skill) => editModal.value?.open(skill);

onMounted(() => {
  skillsStore.loadSkills();
  mcp.loadServers();
});
</script>

<template>
  <n-layout class="skills-view">
    <n-layout-content
      :native-scrollbar="false"
      class="skills-content"
    >
      <div class="skills-container">
        <!-- 页面标题 -->
        <header class="page-header enter-up">
          <span class="eyebrow">Skill</span>
          <h1 class="page-title">
            技能
          </h1>
        </header>

        <SkillList
          @create="openCreate"
          @edit="openEdit"
        />
        <SkillPresetLibrary />
      </div>
    </n-layout-content>

    <SkillEditModal ref="editModal" />
  </n-layout>
</template>

<style scoped lang="scss">
.skills-view {
  height: 100%;
  background: $bg;
}

.skills-content {
  height: 100%;
}

/* 宽屏/超宽屏下分级放宽，配合 SkillPresetLibrary 里 .preset-grid 的 auto-fill
   让预设卡片自动增加列数，而不是永远挤成两列 */
.skills-container {
  max-width: 900px;
  margin: 0 auto;
  padding: 5rem 2rem 8rem;

  @media (min-width: $bp-wide) {
    max-width: 1300px;
  }

  @media (min-width: $bp-ultrawide) {
    max-width: 1600px;
  }
}

.page-header {
  margin-bottom: 4rem;
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.page-title {
  font-family: $font-serif;
  font-size: 2.5rem;
  font-weight: 700;
  line-height: $leading-display;
  color: $ink;
}

/* 卡片样式由 SkillList / SkillPresetLibrary 两个子组件共用，用 :deep 透进去 */
.skills-container {
  :deep(.settings-card) {
    margin-bottom: 20px;
    background: $bg;
    border: $border-soft;
    transition:
      transform $duration $ease,
      box-shadow $duration $ease;

    &:hover {
      transform: translateY(-4px);
      box-shadow: $shadow-hover;
    }
  }

  :deep(.card-header) {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 16px;
    font-weight: 600;

    .n-button {
      margin-left: auto;
    }
  }
}
</style>
