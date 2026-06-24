<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  SkillsView.vue - Skill (技能) 管理视图组件

  功能说明:
  - Skill 列表管理 (创建、编辑、删除、启用/禁用)
  - 绑定 MCP 服务器 (激活 Skill 时一并带入其工具)
  - 资源文件管理 (添加/删除/预览，类似 Claude Agent Skills 的辅助文件)
-->

<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import {
  NLayout,
  NLayoutContent,
  NCard,
  NButton,
  NList,
  NListItem,
  NThing,
  NTag,
  NText,
  NEmpty,
  NModal,
  NForm,
  NFormItem,
  NInput,
  NSelect,
  NSwitch,
  NSpace,
  NPopconfirm,
  NIcon,
  useMessage,
} from "naive-ui";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import {
  Add,
  TrashOutline,
  ExtensionPuzzleOutline,
  DocumentTextOutline,
  CloudUploadOutline,
} from "@vicons/ionicons5";
import { useSkillsStore, type Skill, type SkillDraft } from "@/stores/skills";
import { useMCPStore } from "@/stores/mcp";

// ============ 状态管理 ============

const skillsStore = useSkillsStore();
const mcp = useMCPStore();
const message = useMessage();

// ============ 本地状态 ============

const showEditModal = ref(false);
const saving = ref(false);
const editingId = ref(""); // 空字符串表示新建

const emptyDraft = (): SkillDraft => ({
  id: "",
  name: "",
  description: "",
  instructions: "",
  boundMcpServerIds: [],
  enabled: true,
  resourceFiles: [],
});

const form = ref<SkillDraft>(emptyDraft());

// ============ 计算属性 ============

const mcpServerOptions = computed(() =>
  mcp.servers.map((s) => ({ label: s.name, value: s.id }))
);

const isNew = computed(() => editingId.value === "");

// ============ 方法函数 ============

onMounted(() => {
  skillsStore.loadSkills();
  mcp.loadServers();
});

const getBoundServerName = (serverId: string): string => {
  return mcp.servers.find((s) => s.id === serverId)?.name || "未知服务";
};

const handleCreate = () => {
  editingId.value = "";
  form.value = emptyDraft();
  showEditModal.value = true;
};

const handleEdit = (skill: Skill) => {
  editingId.value = skill.id;
  form.value = {
    id: skill.id,
    name: skill.name,
    description: skill.description,
    instructions: skill.instructions,
    boundMcpServerIds: [...skill.boundMcpServerIds],
    enabled: skill.enabled,
    resourceFiles: [...skill.resourceFiles],
  };
  showEditModal.value = true;
};

const handleSave = async () => {
  if (!form.value.name.trim()) {
    message.error("请输入 Skill 名称");
    return;
  }
  if (!form.value.instructions.trim()) {
    message.error("请输入指令内容");
    return;
  }

  saving.value = true;
  const result = await skillsStore.saveSkill(form.value);
  saving.value = false;

  if (result) {
    // 新建后把 editingId 指向真实 id，方便接着添加资源文件
    editingId.value = result.id;
    form.value.id = result.id;
    message.success(isNew.value ? "Skill 创建成功" : "Skill 已更新");
  } else {
    message.error("保存失败");
  }
};

const handleCloseModal = () => {
  showEditModal.value = false;
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

/** 添加资源文件 -- 必须先保存过一次 Skill (有真实 id) 才能添加 */
const handleAddResourceFile = async () => {
  if (!editingId.value) {
    message.warning("请先保存 Skill，再添加资源文件");
    return;
  }

  const selected = await openFileDialog({ multiple: false });
  if (!selected || typeof selected !== "string") return;

  const updated = await skillsStore.addResourceFile(editingId.value, selected);
  if (updated) {
    form.value.resourceFiles = [...updated.resourceFiles];
    message.success("资源文件已添加");
  } else {
    message.error("添加资源文件失败（请确认文件是文本文件）");
  }
};

const handleRemoveResourceFile = async (filename: string) => {
  const updated = await skillsStore.removeResourceFile(editingId.value, filename);
  if (updated) {
    form.value.resourceFiles = [...updated.resourceFiles];
  }
};
</script>

<template>
  <n-layout class="skills-view">
    <n-layout-content
      :native-scrollbar="false"
      class="skills-content"
    >
      <div class="skills-container">
        <!-- 页面标题 -->
        <h1 class="page-title">
          <n-icon
            :size="28"
            style="margin-right: 12px"
          >
            <ExtensionPuzzleOutline />
          </n-icon>
          Skill
        </h1>

        <!-- Skill 列表卡片 -->
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
                @click="handleCreate"
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
                      @click="handleEdit(skill)"
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
              <n-button @click="handleCreate">
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
      </div>
    </n-layout-content>

    <!-- 创建/编辑 Skill 弹窗 -->
    <n-modal
      v-model:show="showEditModal"
      :title="isNew ? '新建 Skill' : '编辑 Skill'"
      preset="card"
      style="width: 600px; max-height: 85vh"
      :content-style="{ overflowY: 'auto' }"
      :mask-closable="false"
    >
      <n-form
        label-placement="left"
        label-width="100px"
      >
        <n-form-item
          label="名称"
          required
        >
          <n-input
            v-model:value="form.name"
            placeholder="例如：代码审查助手"
          />
        </n-form-item>

        <n-form-item label="描述">
          <n-input
            v-model:value="form.description"
            type="textarea"
            placeholder="简要说明这个 Skill 是做什么的——模型会根据这段描述自主判断要不要调用它"
            :rows="2"
          />
        </n-form-item>

        <n-form-item
          label="指令内容"
          required
        >
          <n-input
            v-model:value="form.instructions"
            type="textarea"
            placeholder="激活这个 Skill 时注入给模型的具体指令（类似 SKILL.md 正文）"
            :rows="6"
          />
        </n-form-item>

        <n-form-item label="绑定 MCP 工具">
          <n-select
            v-model:value="form.boundMcpServerIds"
            multiple
            :options="mcpServerOptions"
            placeholder="激活该 Skill 时一并带入这些服务器的工具（即使全局 MCP 关闭）"
          />
        </n-form-item>

        <n-form-item label="资源文件">
          <n-space vertical style="width: 100%">
            <n-space
              v-if="form.resourceFiles.length > 0"
              vertical
              size="small"
            >
              <n-tag
                v-for="filename in form.resourceFiles"
                :key="filename"
                closable
                @close="handleRemoveResourceFile(filename)"
              >
                {{ filename }}
              </n-tag>
            </n-space>
            <n-button
              size="small"
              :disabled="isNew"
              @click="handleAddResourceFile"
            >
              <template #icon>
                <n-icon><CloudUploadOutline /></n-icon>
              </template>
              添加资源文件
            </n-button>
            <n-text
              v-if="isNew"
              depth="3"
              style="font-size: 12px"
            >
              请先保存 Skill，再添加资源文件
            </n-text>
          </n-space>
        </n-form-item>

        <n-form-item label="启用">
          <n-switch v-model:value="form.enabled" />
        </n-form-item>
      </n-form>

      <template #footer>
        <n-space justify="end">
          <n-button @click="handleCloseModal">
            关闭
          </n-button>
          <n-button
            type="primary"
            :loading="saving"
            @click="handleSave"
          >
            保存
          </n-button>
        </n-space>
      </template>
    </n-modal>
  </n-layout>
</template>

<style scoped lang="scss">
.skills-view {
  height: 100%;
  background: var(--n-color);
}

.skills-content {
  height: 100%;
}

.skills-container {
  max-width: 900px;
  margin: 0 auto;
  padding: 40px 32px;
}

.page-title {
  font-size: 28px;
  font-weight: 600;
  margin-bottom: 32px;
  display: flex;
  align-items: center;
  color: var(--n-text-color-1);
}

.settings-card {
  margin-bottom: 20px;
  border-radius: $radius-xl;
  background: var(--n-color-embed);
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.04);
}

.card-header {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 16px;
  font-weight: 600;

  .n-button {
    margin-left: auto;
  }
}
</style>
