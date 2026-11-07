<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  SkillList.vue - 已配置的 Skill 列表卡片

  列表展示 + 启用/禁用 + 删除；"新建/编辑"交给父组件打开 SkillEditModal。
-->

<script setup lang="ts">
import {
  NCard,
  NButton,
  NList,
  NListItem,
  NThing,
  NTag,
  NText,
  NEmpty,
  NPopconfirm,
  NSpace,
  NSwitch,
  NIcon,
} from "naive-ui";
import { useMessage } from "@/composables/useNotify";
import {
  Add,
  TrashOutline,
  ExtensionPuzzleOutline,
  DocumentTextOutline,
} from "@vicons/ionicons5";
import { useSkillsStore, type Skill } from "@/stores/skills";
import { useMCPStore } from "@/stores/mcp";

const emit = defineEmits<{
  (e: "create"): void;
  (e: "edit", skill: Skill): void;
}>();

const skillsStore = useSkillsStore();
const mcp = useMCPStore();
const message = useMessage();

const getBoundServerName = (serverId: string): string => {
  return mcp.servers.find((s) => s.id === serverId)?.name || "未知服务";
};

const handleDelete = async (skill: Skill) => {
  const success = await skillsStore.deleteSkill(skill.id);
  if (success) {
    message.success("删除成功");
  } else {
    message.error("删除失败");
  }
};

const handleToggleEnabled = async (skill: Skill) => {
  await skillsStore.toggleSkillEnabled(skill);
};
</script>

<template>
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
          <ExtensionPuzzleOutline />
        </n-icon>
        <span>已配置的 Skill</span>
        <n-button
          type="primary"
          size="small"
          @click="emit('create')"
        >
          <template #icon>
            <n-icon><Add /></n-icon>
          </template>
          新建 Skill
        </n-button>
      </div>
    </template>

    <n-list
      v-if="skillsStore.skills.length > 0"
      hoverable
    >
      <n-list-item
        v-for="skill in skillsStore.skills"
        :key="skill.id"
      >
        <n-thing>
          <template #header>
            <n-space align="center">
              <span>{{ skill.name }}</span>
              <n-tag
                :type="skill.enabled ? 'success' : 'default'"
                size="small"
              >
                {{ skill.enabled ? "已启用" : "已禁用" }}
              </n-tag>
            </n-space>
          </template>
          <template #description>
            <n-space
              vertical
              size="small"
            >
              <n-text depth="3">
                {{ skill.description || "无描述" }}
              </n-text>
              <n-space
                v-if="skill.boundMcpServerIds.length > 0 || skill.resourceFiles.length > 0"
                size="small"
              >
                <n-tag
                  v-for="serverId in skill.boundMcpServerIds"
                  :key="serverId"
                  size="small"
                  type="info"
                >
                  工具: {{ getBoundServerName(serverId) }}
                </n-tag>
                <n-tag
                  v-if="skill.resourceFiles.length > 0"
                  size="small"
                >
                  {{ skill.resourceFiles.length }} 个资源文件
                </n-tag>
              </n-space>
            </n-space>
          </template>
          <template #header-extra>
            <n-space>
              <n-switch
                :value="skill.enabled"
                size="small"
                @update:value="handleToggleEnabled(skill)"
              />
              <n-button
                quaternary
                circle
                size="small"
                @click="emit('edit', skill)"
              >
                <template #icon>
                  <n-icon><DocumentTextOutline /></n-icon>
                </template>
              </n-button>
              <n-popconfirm
                positive-text="删除"
                negative-text="取消"
                @positive-click="handleDelete(skill)"
              >
                <template #trigger>
                  <n-button
                    quaternary
                    circle
                    size="small"
                    type="error"
                  >
                    <template #icon>
                      <n-icon><TrashOutline /></n-icon>
                    </template>
                  </n-button>
                </template>
                确定删除 Skill "{{ skill.name }}"？
              </n-popconfirm>
            </n-space>
          </template>
        </n-thing>
      </n-list-item>
    </n-list>

    <n-empty
      v-else
      description="暂无 Skill"
    >
      <template #extra>
        <n-button @click="emit('create')">
          新建 Skill
        </n-button>
      </template>
    </n-empty>

    <template #footer>
      <n-text
        depth="3"
        style="font-size: 12px"
      >
        Skill 可以在 Chat 输入框旁手动选择激活，也可以开启"模型自主判断"让模型根据名称和描述自行决定是否调用
      </n-text>
    </template>
  </n-card>
</template>
