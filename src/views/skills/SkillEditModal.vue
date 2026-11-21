<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  SkillEditModal.vue - 创建/编辑 Skill 弹窗

  父组件通过 ref 调用 open() 新建、open(skill) 编辑。表单状态、保存和资源文件管理都在这里。
-->

<script setup lang="ts">
import { ref, computed } from "vue";
import {
  NModal,
  NForm,
  NFormItem,
  NInput,
  NSelect,
  NSwitch,
  NSpace,
  NButton,
  NTag,
  NText,
  NIcon,
} from "naive-ui";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useMessage } from "@/composables/useNotify";
import { CloudUploadOutline } from "@vicons/ionicons5";
import { useSkillsStore, type Skill, type SkillDraft } from "@/stores/skills";
import { useMCPStore } from "@/stores/mcp";

const skillsStore = useSkillsStore();
const mcp = useMCPStore();
const message = useMessage();

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

const mcpServerOptions = computed(() =>
  mcp.servers.map((s) => ({ label: s.name, value: s.id }))
);

const isNew = computed(() => editingId.value === "");

/** 不传参数 = 新建；传入 skill = 编辑 */
const open = (skill?: Skill) => {
  if (skill) {
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
  } else {
    editingId.value = "";
    form.value = emptyDraft();
  }
  showEditModal.value = true;
};

defineExpose({ open });

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
</template>
