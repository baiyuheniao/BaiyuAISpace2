<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  SettingsView.vue - 设置视图组件
  
  功能说明:
  - 管理 LLM API 配置 (创建、编辑、删除、激活)
  - 管理 Embedding API 配置 (用于知识库向量化)
  - 外观设置 (深色/浅色主题切换)
  - 显示应用版本和关于信息

  主要组成部分:
  - LLM API 配置卡片
  - Embedding API 配置卡片
  - 外观设置卡片
  - 关于信息卡片
  - 新建/编辑弹窗表单
-->

<script setup lang="ts">
import { ref, computed } from "vue";
import { 
  NLayout, 
  NLayoutContent, 
  NCard, 
  NForm, 
  NFormItem, 
  NSelect, 
  NInput, 
  NSwitch, 
  NButton,
  NSpace,
  NList,
  NListItem,
  NThing,
  NTag,
  NPopconfirm,
  NModal,
  NIcon,
  NText,
  NEmpty,
  useMessage
} from "naive-ui";
import { 
  useSettingsStore, 
  PRESET_PROVIDERS, 
  type ApiConfig,
  type EmbeddingApiConfig
} from "@/stores/settings";
import { 
  ServerOutline, 
  KeyOutline, 
  ColorPaletteOutline, 
  InformationCircleOutline,
  Add,
  TrashOutline,
  CreateOutline,
  CheckmarkCircle,
  LinkOutline,
  CubeOutline
} from "@vicons/ionicons5";

// ============ 状态管理 ============

// 设置 Store - 管理 API 配置和主题
const settings = useSettingsStore();

// 消息提示 - 用于操作反馈
const message = useMessage();

// ============ 弹窗状态 ============

/** LLM API 配置 - 新建弹窗显示状态 */
const showCreateModal = ref(false);

/** LLM API 配置 - 编辑弹窗显示状态 */
const showEditModal = ref(false);

/** LLM API 配置 - 当前编辑的配置对象 */
const editingConfig = ref<ApiConfig | null>(null);

/** Embedding API 配置 - 新建弹窗显示状态 */
const showEmbeddingCreateModal = ref(false);

/** Embedding API 配置 - 编辑弹窗显示状态 */
const showEmbeddingEditModal = ref(false);

/** Embedding API 配置 - 当前编辑的配置对象 */
const editingEmbeddingConfig = ref<EmbeddingApiConfig | null>(null);

// ============ 表单数据状态 ============

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
});

/**
 * Embedding API 配置表单数据
 * 用于新建和编辑 Embedding API 配置
 */
const embeddingFormData = ref({
  name: "",                  // 配置名称
  provider: "openai",        // 默认使用 OpenAI
  baseUrl: PRESET_PROVIDERS.openai.baseUrl,  // 默认 Base URL
  model: "text-embedding-3-small",  // 默认模型
  apiKey: "",                // API 密钥
});

// ============ 表单方法 ============

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
  };
};

/**
 * 重置 Embedding API 表单数据
 * 恢复到初始状态
 */
const resetEmbeddingForm = () => {
  embeddingFormData.value = {
    name: "",
    provider: "openai",
    baseUrl: PRESET_PROVIDERS.openai.baseUrl,
    model: "",
    apiKey: "",
  };
};

// ============ 弹窗打开方法 ============

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
  };
  showEditModal.value = true;
};

/**
 * 打开新建 Embedding API 配置弹窗
 */
const openEmbeddingCreateModal = () => {
  resetEmbeddingForm();
  showEmbeddingCreateModal.value = true;
};

/**
 * 打开编辑 Embedding API 配置弹窗
 * 
 * @param config - 要编辑的配置对象
 */
const openEmbeddingEditModal = (config: EmbeddingApiConfig) => {
  editingEmbeddingConfig.value = config;
  embeddingFormData.value = {
    name: config.name,
    provider: config.provider,
    baseUrl: config.baseUrl,
    model: config.model,
    apiKey: config.apiKey,
  };
  showEmbeddingEditModal.value = true;
};

// ============ 提供商切换处理 ============

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
 * 处理 Embedding 提供商切换
 * 自动填入对应提供商的默认 Base URL
 * 
 * @param provider - 提供商标识符
 */
const handleEmbeddingProviderChange = (provider: string) => {
  embeddingFormData.value.provider = provider;
  embeddingFormData.value.baseUrl = PRESET_PROVIDERS[provider]?.baseUrl || "";
};

// ============ CRUD 操作处理 ============

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
    formData.value.baseUrl
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

/**
 * 创建新的 Embedding API 配置
 */
const handleEmbeddingCreate = async () => {
  // 表单验证
  if (!embeddingFormData.value.name.trim()) {
    message.error("请输入配置名称");
    return;
  }
  if (!embeddingFormData.value.model.trim()) {
    message.error("请输入 Embedding 模型名称");
    return;
  }
  if (!embeddingFormData.value.apiKey.trim()) {
    message.error("请输入 API Key");
    return;
  }

  // 调用 Store 方法创建配置
  settings.createEmbeddingApiConfig(
    embeddingFormData.value.name,
    embeddingFormData.value.provider,
    embeddingFormData.value.model,
    embeddingFormData.value.apiKey,
    embeddingFormData.value.baseUrl
  );

  // 提示成功并关闭弹窗
  message.success("Embedding API 配置已创建");
  showEmbeddingCreateModal.value = false;
  resetEmbeddingForm();
};

/**
 * 更新 Embedding API 配置
 */
const handleEmbeddingUpdate = async () => {
  if (!editingEmbeddingConfig.value) return;
  
  // 表单验证
  if (!embeddingFormData.value.name.trim()) {
    message.error("请输入配置名称");
    return;
  }
  if (!embeddingFormData.value.model.trim()) {
    message.error("请输入 Embedding 模型名称");
    return;
  }

  // 调用 Store 方法更新配置
  settings.updateEmbeddingApiConfig(editingEmbeddingConfig.value.id, {
    name: embeddingFormData.value.name,
    provider: embeddingFormData.value.provider,
    baseUrl: embeddingFormData.value.baseUrl,
    model: embeddingFormData.value.model,
    apiKey: embeddingFormData.value.apiKey,
  });

  // 提示成功并关闭弹窗
  message.success("Embedding API 配置已更新");
  showEmbeddingEditModal.value = false;
  editingEmbeddingConfig.value = null;
};

/**
 * 删除 Embedding API 配置
 * 
 * @param configId - 要删除的配置 ID
 */
const handleEmbeddingDelete = (configId: string) => {
  settings.deleteEmbeddingApiConfig(configId);
  message.success("Embedding API 配置已删除");
};

/**
 * 设置当前使用的 Embedding API 配置
 * 
 * @param configId - 要激活的配置 ID
 */
const handleSetEmbeddingActive = (configId: string) => {
  settings.setActiveEmbeddingApiConfig(configId);
  message.success("已设为当前 Embedding 配置");
};

// ============ 计算属性 ============

/**
 * 提供商下拉选项
 * 从 Store 获取预设的提供商列表
 */
const providerOptions = computed(() => settings.presetProviderOptions);
</script>

<template>
  <!-- 设置主布局容器 -->
  <n-layout class="settings-view">
    <!-- 设置内容区域 -->
    <n-layout-content
      :native-scrollbar="false"
      class="settings-content"
    >
      <div class="settings-container">
        <!-- 页面标题 -->
        <h1 class="page-title">
          <n-icon
            :size="28"
            style="margin-right: 12px;"
          >
            <ServerOutline />
          </n-icon>
          设置
        </h1>

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

        <!-- Embedding API 配置卡片 -->
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
                <DocumentTextOutline />
              </n-icon>
              <span>Embedding 向量模型 API 配置</span>
              <n-button
                type="primary"
                size="small"
                @click="openEmbeddingCreateModal"
              >
                <template #icon>
                  <n-icon><Add /></n-icon>
                </template>
                新建配置
              </n-button>
            </div>
          </template>

          <!-- Embedding 配置列表 -->
          <n-list
            v-if="settings.embeddingApiConfigs.length > 0"
            hoverable
            clickable
          >
            <n-list-item 
              v-for="config in settings.embeddingApiConfigs" 
              :key="config.id"
              @click="handleSetEmbeddingActive(config.id)"
            >
              <n-thing>
                <template #header>
                  <n-space align="center">
                    <span>{{ config.name }}</span>
                    <n-tag 
                      v-if="config.id === settings.activeEmbeddingApiConfigId" 
                      type="success" 
                      size="small"
                    >
                      当前使用
                    </n-tag>
                  </n-space>
                </template>
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
                <template #header-extra>
                  <n-space>
                    <n-button
                      quaternary
                      circle
                      size="small"
                      @click.stop="openEmbeddingEditModal(config)"
                    >
                      <template #icon>
                        <n-icon><CreateOutline /></n-icon>
                      </template>
                    </n-button>
                    <n-popconfirm 
                      positive-text="删除"
                      negative-text="取消"
                      @positive-click="handleEmbeddingDelete(config.id)"
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
                      确定删除 Embedding API 配置 "{{ config.name }}"？
                    </n-popconfirm>
                  </n-space>
                </template>
              </n-thing>
            </n-list-item>
          </n-list>

          <n-empty
            v-else
            description="暂无 Embedding API 配置"
          />

          <template
            v-if="settings.embeddingApiConfigs.length > 0"
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
              Embedding API 用于知识库的文档向量化和检索查询
            </n-text>
          </template>
        </n-card>

        <!-- 外观设置卡片 -->
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
                <ColorPaletteOutline />
              </n-icon>
              <span>外观</span>
            </div>
          </template>

          <!-- 表单设置 -->
          <n-form
            label-placement="left"
            label-width="100px"
            class="settings-form"
          >
            <n-form-item label="深色模式">
              <!-- 主题切换开关 -->
              <n-switch
                :value="settings.darkMode"
                size="large"
                @update:value="settings.toggleTheme"
              >
                <template #checked>
                  开启
                </template>
                <template #unchecked>
                  关闭
                </template>
              </n-switch>
            </n-form-item>
          </n-form>
        </n-card>

        <!-- 关于卡片 -->
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
                <InformationCircleOutline />
              </n-icon>
              <span>关于</span>
            </div>
          </template>

          <!-- 关于内容 -->
          <div class="about-content">
            <div class="about-item">
              <span class="about-label">版本</span>
              <n-tag
                type="success"
                size="small"
              >
                v0.1.0
              </n-tag>
            </div>
            <div class="about-item">
              <span class="about-label">许可证</span>
              <n-tag
                type="info"
                size="small"
              >
                MPL-2.0
              </n-tag>
            </div>
            <div class="about-item">
              <span class="about-label">GitHub</span>
              <n-text
                underline
                class="about-link"
              >
                baiyuheniao/BaiyuAISpace2
              </n-text>
            </div>
          </div>
        </n-card>

        <!-- 页脚 -->
        <div class="footer-text">
          <n-text
            depth="3"
            style="font-size: 12px;"
          >
            Made with ❤️ by Baiyu
          </n-text>
        </div>
      </div>
    </n-layout-content>

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
              style="font-size: 12px; color: #f0a020;"
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

    <!-- 新建 Embedding API 配置弹窗 -->
    <n-modal
      v-model:show="showEmbeddingCreateModal"
      title="新建 Embedding API 配置"
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
            v-model:value="embeddingFormData.name" 
            placeholder="例如：OpenAI Embedding"
          />
        </n-form-item>

        <n-form-item
          label="服务商"
          required
        >
          <n-select
            :value="embeddingFormData.provider"
            :options="providerOptions"
            placeholder="选择服务商"
            @update:value="handleEmbeddingProviderChange"
          />
        </n-form-item>

        <n-form-item
          label="Base URL"
          required
        >
          <n-input 
            v-model:value="embeddingFormData.baseUrl" 
            placeholder="https://api.openai.com/v1"
          />
          <template #feedback>
            <n-text
              depth="3"
              style="font-size: 12px;"
            >
              已自动填入 {{ PRESET_PROVIDERS[embeddingFormData.provider]?.name }} 默认地址
            </n-text>
          </template>
        </n-form-item>

        <n-form-item
          label="Embedding 模型"
          required
        >
          <n-input 
            v-model:value="embeddingFormData.model" 
            placeholder="例如：text-embedding-3-small, embedding-2, bge-large-zh..."
          />
          <template #feedback>
            <n-text
              depth="3"
              style="font-size: 12px;"
            >
              输入 Embedding 模型名称，可参考服务商官方文档
            </n-text>
          </template>
        </n-form-item>

        <n-form-item
          label="API Key"
          required
        >
          <n-input 
            v-model:value="embeddingFormData.apiKey" 
            type="password"
            show-password-on="click"
            placeholder="输入 API Key"
          />
        </n-form-item>
      </n-form>

      <template #footer>
        <n-space justify="end">
          <n-button @click="showEmbeddingCreateModal = false">
            取消
          </n-button>
          <n-button
            type="primary"
            @click="handleEmbeddingCreate"
          >
            创建
          </n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- 编辑 Embedding API 配置弹窗 -->
    <n-modal
      v-model:show="showEmbeddingEditModal"
      title="编辑 Embedding API 配置"
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
            v-model:value="embeddingFormData.name" 
            placeholder="例如：OpenAI Embedding"
          />
        </n-form-item>

        <n-form-item
          label="服务商"
          required
        >
          <n-select
            :value="embeddingFormData.provider"
            :options="providerOptions"
            placeholder="选择服务商"
            @update:value="handleEmbeddingProviderChange"
          />
        </n-form-item>

        <n-form-item
          label="Base URL"
          required
        >
          <n-input 
            v-model:value="embeddingFormData.baseUrl" 
            placeholder="https://api.openai.com/v1"
          />
        </n-form-item>

        <n-form-item
          label="Embedding 模型"
          required
        >
          <n-input 
            v-model:value="embeddingFormData.model" 
            placeholder="例如：text-embedding-3-small, embedding-2..."
          />
        </n-form-item>

        <n-form-item label="API Key">
          <n-input 
            v-model:value="embeddingFormData.apiKey" 
            type="password"
            show-password-on="click"
            placeholder="留空表示不修改"
          />
          <template #feedback>
            <n-text
              depth="3"
              style="font-size: 12px;"
            >
              留空表示保持原 API Key 不变
            </n-text>
          </template>
        </n-form-item>
      </n-form>

      <template #footer>
        <n-space justify="end">
          <n-button @click="showEmbeddingEditModal = false">
            取消
          </n-button>
          <n-button
            type="primary"
            @click="handleEmbeddingUpdate"
          >
            保存
          </n-button>
        </n-space>
      </template>
    </n-modal>
  </n-layout>
</template>

<style scoped lang="scss">
/* 设置主容器 */
.settings-view {
  height: 100%;
  background: var(--n-color);
}

/* 设置内容区域 */
.settings-content {
  height: 100%;
}

/* 内容容器 - 限制最大宽度并居中 */
.settings-container {
  max-width: 700px;
  margin: 0 auto;
  padding: 40px 32px;
}

/* 页面标题样式 */
.page-title {
  font-size: 28px;
  font-weight: 600;
  margin-bottom: 32px;
  display: flex;
  align-items: center;
  color: var(--n-text-color-1);
}

/* 设置卡片样式 */
.settings-card {
  margin-bottom: 20px;
  border-radius: 16px;
  background: var(--n-color-embed);
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.04);
}

/* 卡片标题样式 */
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
