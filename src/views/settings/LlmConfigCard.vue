<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  LlmConfigCard.vue - 对话模型 API 配置卡片（列表 + 新建/编辑弹窗）
  （从 SettingsView.vue 拆出，逻辑与模板原样搬迁。）
-->

<script setup lang="ts">
import { ref, computed } from "vue";
import { NCard, NForm, NFormItem, NSelect, NInput, NInputNumber, NButton, NSpace, NList, NListItem, NThing, NTag, NPopconfirm, NModal, NIcon, NText, NEmpty } from "naive-ui";
import { useMessage } from "@/composables/useNotify";
import {
  useSettingsStore,
  PRESET_PROVIDERS,
  type ApiConfig,
} from "@/stores/settings";
import {
  KeyOutline,
  Add,
  TrashOutline,
  CreateOutline,
  CheckmarkCircle,
  LinkOutline,
  CubeOutline,
} from "@vicons/ionicons5";

const settings = useSettingsStore();
const message = useMessage();

/** LLM API 配置 - 新建弹窗显示状态 */
const showCreateModal = ref(false);

/** LLM API 配置 - 编辑弹窗显示状态 */
const showEditModal = ref(false);

/** LLM API 配置 - 当前编辑的配置对象 */
const editingConfig = ref<ApiConfig | null>(null);

/**
 * LLM API 配置表单数据
 * 用于新建和编辑 LLM API 配置
 */
const formData = ref({
  name: "",                  // 配置名称
  provider: "openai",        // 默认使用 OpenAI
  baseUrl: PRESET_PROVIDERS.openai.baseUrl,  // 默认 Base URL
  model: "",                 // 模型名称
  apiKey: "",                // API 密钥
  maxTokens: null as number | null,  // 最大输出 token 数（null = 后端默认值）
});

/**
 * 重置 LLM API 表单数据
 * 恢复到初始状态
 */
const resetForm = () => {
  formData.value = {
    name: "",
    provider: "openai",
    baseUrl: PRESET_PROVIDERS.openai.baseUrl,
    model: "",
    apiKey: "",
    maxTokens: null,
  };
};

/**
 * 打开新建 LLM API 配置弹窗
 * 先重置表单，再显示弹窗
 */
const openCreateModal = () => {
  resetForm();
  showCreateModal.value = true;
};

/**
 * 打开编辑 LLM API 配置弹窗
 * 填充当前配置数据到表单
 * 
 * @param config - 要编辑的配置对象
 */
const openEditModal = (config: ApiConfig) => {
  editingConfig.value = config;
  formData.value = {
    name: config.name,
    provider: config.provider,
    baseUrl: config.baseUrl,
    model: config.model,
    apiKey: config.apiKey,
    maxTokens: config.maxTokens ?? null,
  };
  showEditModal.value = true;
};

/**
 * 处理 LLM 提供商切换
 * 自动填入对应提供商的默认 Base URL
 * 
 * @param provider - 提供商标识符
 */
const handleProviderChange = (provider: string) => {
  formData.value.provider = provider;
  formData.value.baseUrl = PRESET_PROVIDERS[provider]?.baseUrl || "";
};

/**
 * 创建新的 LLM API 配置
 * 验证表单数据后调用 Store 方法保存
 */
const handleCreate = async () => {
  // 表单验证
  if (!formData.value.name.trim()) {
    message.error("请输入配置名称");
    return;
  }
  if (!formData.value.model.trim()) {
    message.error("请输入模型名称");
    return;
  }
  if (!formData.value.apiKey.trim()) {
    message.error("请输入 API Key");
    return;
  }

  // 调用 Store 方法创建配置
  settings.createApiConfig(
    formData.value.name,
    formData.value.provider,
    formData.value.model,
    formData.value.apiKey,
    formData.value.baseUrl,
    formData.value.maxTokens ?? undefined
  );

  // 提示成功并关闭弹窗
  message.success("API 配置已创建");
  showCreateModal.value = false;
  resetForm();
};

/**
 * 更新 LLM API 配置
 * 验证表单数据后调用 Store 方法保存
 */
const handleUpdate = async () => {
  if (!editingConfig.value) return;
  
  // 表单验证
  if (!formData.value.name.trim()) {
    message.error("请输入配置名称");
    return;
  }
  if (!formData.value.model.trim()) {
    message.error("请输入模型名称");
    return;
  }

  // 调用 Store 方法更新配置
  settings.updateApiConfig(editingConfig.value.id, {
    name: formData.value.name,
    provider: formData.value.provider,
    baseUrl: formData.value.baseUrl,
    model: formData.value.model,
    apiKey: formData.value.apiKey,
    maxTokens: formData.value.maxTokens ?? undefined,
  });

  // 提示成功并关闭弹窗
  message.success("API 配置已更新");
  showEditModal.value = false;
  editingConfig.value = null;
};

/**
 * 删除 LLM API 配置
 * 
 * @param configId - 要删除的配置 ID
 */
const handleDelete = (configId: string) => {
  settings.deleteApiConfig(configId);
  message.success("API 配置已删除");
};

/**
 * 设置当前使用的 LLM API 配置
 * 
 * @param configId - 要激活的配置 ID
 */
const handleSetActive = (configId: string) => {
  settings.setActiveConfig(configId);
  message.success("已设为当前使用配置");
};

const providerOptions = computed(() => settings.presetProviderOptions);
</script>

<template>
  <!-- LLM API 配置卡片 -->
  <n-card
    class="settings-card"
    :bordered="false"
  >
    <!-- 卡片标题 -->
    <template #header>
      <div class="card-header">
        <n-icon
          :size="20"
          depth="3"
        >
          <KeyOutline />
        </n-icon>
        <span>对话模型 API 配置</span>
        <!-- 新建配置按钮 -->
        <n-button
          type="primary"
          size="small"
          @click="openCreateModal"
        >
          <template #icon>
            <n-icon><Add /></n-icon>
          </template>
          新建配置
        </n-button>
      </div>
    </template>

    <!-- 配置列表 -->
    <n-list
      v-if="settings.apiConfigs.length > 0"
      hoverable
      clickable
    >
      <!-- 遍历显示每个配置 -->
      <n-list-item 
        v-for="config in settings.apiConfigs" 
        :key="config.id"
        @click="handleSetActive(config.id)"
      >
        <n-thing>
          <!-- 配置名称 -->
          <template #header>
            <n-space align="center">
              <span>{{ config.name }}</span>
              <!-- 当前使用标签 -->
              <n-tag 
                v-if="config.id === settings.activeConfigId" 
                type="success" 
                size="small"
              >
                当前使用
              </n-tag>
            </n-space>
          </template>
          
          <!-- 配置描述 -->
          <template #description>
            <n-space
              vertical
              size="small"
            >
              <n-text depth="3">
                <n-icon
                  :size="14"
                  style="margin-right: 4px;"
                >
                  <CubeOutline />
                </n-icon>
                模型: {{ config.model }}
              </n-text>
              <n-text depth="3">
                <n-icon
                  :size="14"
                  style="margin-right: 4px;"
                >
                  <LinkOutline />
                </n-icon>
                {{ PRESET_PROVIDERS[config.provider]?.name || config.provider }}
              </n-text>
            </n-space>
          </template>
          
          <!-- 操作按钮 -->
          <template #header-extra>
            <n-space>
              <!-- 编辑按钮 -->
              <n-button
                quaternary
                circle
                size="small"
                @click.stop="openEditModal(config)"
              >
                <template #icon>
                  <n-icon><CreateOutline /></n-icon>
                </template>
              </n-button>
              <!-- 删除按钮 -->
              <n-popconfirm 
                positive-text="删除"
                negative-text="取消"
                @positive-click="handleDelete(config.id)"
              >
                <template #trigger>
                  <n-button
                    quaternary
                    circle
                    size="small"
                    type="error"
                    @click.stop
                  >
                    <template #icon>
                      <n-icon><TrashOutline /></n-icon>
                    </template>
                  </n-button>
                </template>
                确定删除配置 "{{ config.name }}"？
              </n-popconfirm>
            </n-space>
          </template>
        </n-thing>
      </n-list-item>
    </n-list>

    <!-- 空状态 -->
    <n-empty
      v-else
      description="暂无 API 配置"
    />

    <!-- 卡片底部提示 -->
    <template
      v-if="settings.apiConfigs.length > 0"
      #footer
    >
      <n-text
        depth="3"
        style="font-size: 12px;"
      >
        <n-icon
          :size="12"
          style="margin-right: 4px;"
        >
          <CheckmarkCircle />
        </n-icon>
        API Key 使用系统密钥链加密存储（Windows Credential / macOS Keychain / Linux Secret Service）
      </n-text>
    </template>
  </n-card>

  <!-- 新建 LLM API 配置弹窗 -->
  <n-modal
    v-model:show="showCreateModal"
    title="新建 API 配置"
    preset="card"
    style="width: 500px"
    :mask-closable="false"
  >
    <n-form
      label-placement="left"
      label-width="100px"
    >
      <n-form-item
        label="配置名称"
        required
      >
        <n-input 
          v-model:value="formData.name" 
          placeholder="例如：OpenAI 生产环境"
        />
      </n-form-item>

      <n-form-item
        label="服务商"
        required
      >
        <n-select
          :value="formData.provider"
          :options="providerOptions"
          placeholder="选择服务商"
          @update:value="handleProviderChange"
        />
      </n-form-item>

      <n-form-item
        label="Base URL"
        required
      >
        <n-input 
          v-model:value="formData.baseUrl" 
          placeholder="https://api.example.com/v1"
        />
        <template #feedback>
          <n-text
            depth="3"
            style="font-size: 12px;"
          >
            已自动填入 {{ PRESET_PROVIDERS[formData.provider]?.name }} 默认地址，可手动修改
          </n-text>
        </template>
      </n-form-item>

      <n-form-item
        label="模型"
        required
      >
        <n-input 
          v-model:value="formData.model" 
          placeholder="例如：gpt-4o, claude-3-5-sonnet, qwen-max..."
        />
        <template #feedback>
          <n-text
            depth="3"
            style="font-size: 12px;"
          >
            输入模型名称，可参考服务商官方文档
          </n-text>
        </template>
      </n-form-item>

      <n-form-item
        label="API Key"
        required
      >
        <n-input
          v-model:value="formData.apiKey"
          type="password"
          show-password-on="click"
          placeholder="输入 API Key"
        />
      </n-form-item>

      <n-form-item label="Max Tokens">
        <n-input-number
          v-model:value="formData.maxTokens"
          :min="1"
          :max="1000000"
          placeholder="留空则不限制（Anthropic 除外）"
          style="width: 100%"
        />
        <template #feedback>
          <n-text depth="3" style="font-size: 12px;">
            留空时大多数服务商不会传这个参数，不会截断长回答。Anthropic 接口强制要求该字段，留空则用 32000。
          </n-text>
        </template>
      </n-form-item>
    </n-form>

    <template #footer>
      <n-space justify="end">
        <n-button @click="showCreateModal = false">
          取消
        </n-button>
        <n-button
          type="primary"
          @click="handleCreate"
        >
          创建
        </n-button>
      </n-space>
    </template>
  </n-modal>

  <!-- 编辑 LLM API 配置弹窗 -->
  <n-modal
    v-model:show="showEditModal"
    title="编辑 API 配置"
    preset="card"
    style="width: 500px"
    :mask-closable="false"
  >
    <n-form
      label-placement="left"
      label-width="100px"
    >
      <n-form-item
        label="配置名称"
        required
      >
        <n-input 
          v-model:value="formData.name" 
          placeholder="例如：OpenAI 生产环境"
        />
      </n-form-item>

      <n-form-item
        label="服务商"
        required
      >
        <n-select
          :value="formData.provider"
          :options="providerOptions"
          placeholder="选择服务商"
          @update:value="handleProviderChange"
        />
      </n-form-item>

      <n-form-item
        label="Base URL"
        required
      >
        <n-input 
          v-model:value="formData.baseUrl" 
          placeholder="https://api.example.com/v1"
        />
      </n-form-item>

      <n-form-item
        label="模型"
        required
      >
        <n-input 
          v-model:value="formData.model" 
          placeholder="例如：gpt-4o, claude-3-5-sonnet..."
        />
      </n-form-item>

      <n-form-item label="API Key">
        <n-input 
          v-model:value="formData.apiKey" 
          type="password"
          show-password-on="click"
          :placeholder="formData.provider === 'baidu' ? '请输入 access_token' : '留空表示不修改'"
        />
        <template #feedback>
          <n-text
            v-if="formData.provider === 'baidu'"
            depth="2"
            style="font-size: 12px; color: var(--color-ink-soft);"
          >
            百度千帆需要 access_token，而非 API Key。请在
            <n-a
              href="https://console.bce.baidu.com/qianfan/"
              target="_blank"
            >
              百度千帆控制台
            </n-a>
            获取 API Key 和 Secret Key，然后
            <n-a
              href="https://cloud.baidu.com/doc/WENXINWORKSHOP/s/Ck3edn42t"
              target="_blank"
            >
              换取 access_token
            </n-a>
          </n-text>
          <n-text
            v-else
            depth="3"
            style="font-size: 12px;"
          >
            留空表示保持原 API Key 不变
          </n-text>
        </template>
      </n-form-item>

      <n-form-item label="Max Tokens">
        <n-input-number
          v-model:value="formData.maxTokens"
          :min="1"
          :max="1000000"
          placeholder="留空则不限制（Anthropic 除外）"
          style="width: 100%"
        />
        <template #feedback>
          <n-text depth="3" style="font-size: 12px;">
            留空时大多数服务商不会传这个参数，不会截断长回答。Anthropic 接口强制要求该字段，留空则用 32000。
          </n-text>
        </template>
      </n-form-item>
    </n-form>

    <template #footer>
      <n-space justify="end">
        <n-button @click="showEditModal = false">
          取消
        </n-button>
        <n-button
          type="primary"
          @click="handleUpdate"
        >
          保存
        </n-button>
      </n-space>
    </template>
  </n-modal>
</template>
