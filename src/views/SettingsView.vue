<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

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

const settings = useSettingsStore();
const message = useMessage();

// LLM API Config Modal state
const showCreateModal = ref(false);
const showEditModal = ref(false);
const editingConfig = ref<ApiConfig | null>(null);

// Embedding API Config Modal state
const showEmbeddingCreateModal = ref(false);
const showEmbeddingEditModal = ref(false);
const editingEmbeddingConfig = ref<EmbeddingApiConfig | null>(null);

// LLM API Form state
const formData = ref({
  name: "",
  provider: "openai",
  baseUrl: PRESET_PROVIDERS.openai.baseUrl,
  model: "",
  apiKey: "",
});

// Embedding API Form state
const embeddingFormData = ref({
  name: "",
  provider: "openai",
  baseUrl: PRESET_PROVIDERS.openai.baseUrl,
  model: "text-embedding-3-small",
  apiKey: "",
});

// Reset LLM API form
const resetForm = () => {
  formData.value = {
    name: "",
    provider: "openai",
    baseUrl: PRESET_PROVIDERS.openai.baseUrl,
    model: "",
    apiKey: "",
  };
};

// Reset Embedding API form
const resetEmbeddingForm = () => {
  embeddingFormData.value = {
    name: "",
    provider: "openai",
    baseUrl: PRESET_PROVIDERS.openai.baseUrl,
    model: "",
    apiKey: "",
  };
};

// Open LLM API create modal
const openCreateModal = () => {
  resetForm();
  showCreateModal.value = true;
};

// Open LLM API edit modal
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

// Open Embedding API create modal
const openEmbeddingCreateModal = () => {
  resetEmbeddingForm();
  showEmbeddingCreateModal.value = true;
};

// Open Embedding API edit modal
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

// Handle LLM provider change - auto fill base URL
const handleProviderChange = (provider: string) => {
  formData.value.provider = provider;
  formData.value.baseUrl = PRESET_PROVIDERS[provider]?.baseUrl || "";
};

// Handle Embedding provider change - auto fill base URL
const handleEmbeddingProviderChange = (provider: string) => {
  embeddingFormData.value.provider = provider;
  embeddingFormData.value.baseUrl = PRESET_PROVIDERS[provider]?.baseUrl || "";
};

// Create new config
const handleCreate = async () => {
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

  settings.createApiConfig(
    formData.value.name,
    formData.value.provider,
    formData.value.model,
    formData.value.apiKey,
    formData.value.baseUrl
  );

  message.success("API 配置已创建");
  showCreateModal.value = false;
  resetForm();
};

// Update config
const handleUpdate = async () => {
  if (!editingConfig.value) return;
  
  if (!formData.value.name.trim()) {
    message.error("请输入配置名称");
    return;
  }
  if (!formData.value.model.trim()) {
    message.error("请输入模型名称");
    return;
  }

  settings.updateApiConfig(editingConfig.value.id, {
    name: formData.value.name,
    provider: formData.value.provider,
    baseUrl: formData.value.baseUrl,
    model: formData.value.model,
    apiKey: formData.value.apiKey,
  });

  message.success("API 配置已更新");
  showEditModal.value = false;
  editingConfig.value = null;
};

// Delete config
const handleDelete = (configId: string) => {
  settings.deleteApiConfig(configId);
  message.success("API 配置已删除");
};

// Set active config
const handleSetActive = (configId: string) => {
  settings.setActiveConfig(configId);
  message.success("已设为当前使用配置");
};

// Create Embedding API config
const handleEmbeddingCreate = async () => {
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

  settings.createEmbeddingApiConfig(
    embeddingFormData.value.name,
    embeddingFormData.value.provider,
    embeddingFormData.value.model,
    embeddingFormData.value.apiKey,
    embeddingFormData.value.baseUrl
  );

  message.success("Embedding API 配置已创建");
  showEmbeddingCreateModal.value = false;
  resetEmbeddingForm();
};

// Update Embedding API config
const handleEmbeddingUpdate = async () => {
  if (!editingEmbeddingConfig.value) return;
  
  if (!embeddingFormData.value.name.trim()) {
    message.error("请输入配置名称");
    return;
  }
  if (!embeddingFormData.value.model.trim()) {
    message.error("请输入 Embedding 模型名称");
    return;
  }

  settings.updateEmbeddingApiConfig(editingEmbeddingConfig.value.id, {
    name: embeddingFormData.value.name,
    provider: embeddingFormData.value.provider,
    baseUrl: embeddingFormData.value.baseUrl,
    model: embeddingFormData.value.model,
    apiKey: embeddingFormData.value.apiKey,
  });

  message.success("Embedding API 配置已更新");
  showEmbeddingEditModal.value = false;
  editingEmbeddingConfig.value = null;
};

// Delete Embedding API config
const handleEmbeddingDelete = (configId: string) => {
  settings.deleteEmbeddingApiConfig(configId);
  message.success("Embedding API 配置已删除");
};

// Set active Embedding API config
const handleSetEmbeddingActive = (configId: string) => {
  settings.setActiveEmbeddingApiConfig(configId);
  message.success("已设为当前 Embedding 配置");
};

// Provider options
const providerOptions = computed(() => settings.presetProviderOptions);
</script>

<template>
  <n-layout class="settings-view">
    <n-layout-content :native-scrollbar="false" class="settings-content">
      <div class="settings-container">
        <h1 class="page-title">
          <n-icon :size="28" style="margin-right: 12px;"><ServerOutline /></n-icon>
          设置
        </h1>

        <!-- LLM API Configurations -->
        <n-card class="settings-card" :bordered="false">
          <template #header>
            <div class="card-header">
              <n-icon :size="20" depth="3"><KeyOutline /></n-icon>
              <span>对话模型 API 配置</span>
              <n-button type="primary" size="small" @click="openCreateModal">
                <template #icon>
                  <n-icon><Add /></n-icon>
                </template>
                新建配置
              </n-button>
            </div>
          </template>

          <!-- Config List -->
          <n-list v-if="settings.apiConfigs.length > 0" hoverable clickable>
            <n-list-item 
              v-for="config in settings.apiConfigs" 
              :key="config.id"
              @click="handleSetActive(config.id)"
            >
              <n-thing>
                <template #header>
                  <n-space align="center">
                    <span>{{ config.name }}</span>
                    <n-tag 
                      v-if="config.id === settings.activeConfigId" 
                      type="success" 
                      size="small"
                    >
                      当前使用
                    </n-tag>
                  </n-space>
                </template>
                <template #description>
                  <n-space vertical size="small">
                    <n-text depth="3">
                      <n-icon :size="14" style="margin-right: 4px;"><CubeOutline /></n-icon>
                      模型: {{ config.model }}
                    </n-text>
                    <n-text depth="3">
                      <n-icon :size="14" style="margin-right: 4px;"><LinkOutline /></n-icon>
                      {{ PRESET_PROVIDERS[config.provider]?.name || config.provider }}
                    </n-text>
                  </n-space>
                </template>
                <template #header-extra>
                  <n-space>
                    <n-button quaternary circle size="small" @click.stop="openEditModal(config)">
                      <template #icon>
                        <n-icon><CreateOutline /></n-icon>
                      </template>
                    </n-button>
                    <n-popconfirm 
                      @positive-click="handleDelete(config.id)"
                      positive-text="删除"
                      negative-text="取消"
                    >
                      <template #trigger>
                        <n-button quaternary circle size="small" type="error" @click.stop>
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

          <n-empty v-else description="暂无 API 配置" />

          <template #footer v-if="settings.apiConfigs.length > 0">
            <n-text depth="3" style="font-size: 12px;">
              <n-icon :size="12" style="margin-right: 4px;"><CheckmarkCircle /></n-icon>
              API Key 使用系统密钥链加密存储（Windows Credential / macOS Keychain / Linux Secret Service）
            </n-text>
          </template>
        </n-card>

        <!-- Embedding API Configurations -->
        <n-card class="settings-card" :bordered="false">
          <template #header>
            <div class="card-header">
              <n-icon :size="20" depth="3"><DocumentTextOutline /></n-icon>
              <span>Embedding 向量模型 API 配置</span>
              <n-button type="primary" size="small" @click="openEmbeddingCreateModal">
                <template #icon>
                  <n-icon><Add /></n-icon>
                </template>
                新建配置
              </n-button>
            </div>
          </template>

          <!-- Embedding Config List -->
          <n-list v-if="settings.embeddingApiConfigs.length > 0" hoverable clickable>
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
                  <n-space vertical size="small">
                    <n-text depth="3">
                      <n-icon :size="14" style="margin-right: 4px;"><CubeOutline /></n-icon>
                      模型: {{ config.model }}
                    </n-text>
                    <n-text depth="3">
                      <n-icon :size="14" style="margin-right: 4px;"><LinkOutline /></n-icon>
                      {{ PRESET_PROVIDERS[config.provider]?.name || config.provider }}
                    </n-text>
                  </n-space>
                </template>
                <template #header-extra>
                  <n-space>
                    <n-button quaternary circle size="small" @click.stop="openEmbeddingEditModal(config)">
                      <template #icon>
                        <n-icon><CreateOutline /></n-icon>
                      </template>
                    </n-button>
                    <n-popconfirm 
                      @positive-click="handleEmbeddingDelete(config.id)"
                      positive-text="删除"
                      negative-text="取消"
                    >
                      <template #trigger>
                        <n-button quaternary circle size="small" type="error" @click.stop>
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

          <n-empty v-else description="暂无 Embedding API 配置" />

          <template #footer v-if="settings.embeddingApiConfigs.length > 0">
            <n-text depth="3" style="font-size: 12px;">
              <n-icon :size="12" style="margin-right: 4px;"><CheckmarkCircle /></n-icon>
              Embedding API 用于知识库的文档向量化和检索查询
            </n-text>
          </template>
        </n-card>

        <!-- Appearance Settings -->
        <n-card class="settings-card" :bordered="false">
          <template #header>
            <div class="card-header">
              <n-icon :size="20" depth="3"><ColorPaletteOutline /></n-icon>
              <span>外观</span>
            </div>
          </template>

          <n-form label-placement="left" label-width="100px" class="settings-form">
            <n-form-item label="深色模式">
              <n-switch
                :value="settings.darkMode"
                @update:value="settings.toggleTheme"
                size="large"
              >
                <template #checked>开启</template>
                <template #unchecked>关闭</template>
              </n-switch>
            </n-form-item>
          </n-form>
        </n-card>

        <!-- About -->
        <n-card class="settings-card" :bordered="false">
          <template #header>
            <div class="card-header">
              <n-icon :size="20" depth="3"><InformationCircleOutline /></n-icon>
              <span>关于</span>
            </div>
          </template>

          <div class="about-content">
            <div class="about-item">
              <span class="about-label">版本</span>
              <n-tag type="success" size="small">v0.1.0</n-tag>
            </div>
            <div class="about-item">
              <span class="about-label">许可证</span>
              <n-tag type="info" size="small">MPL-2.0</n-tag>
            </div>
            <div class="about-item">
              <span class="about-label">GitHub</span>
              <n-text underline class="about-link">
                baiyuheniao/BaiyuAISpace2
              </n-text>
            </div>
          </div>
        </n-card>

        <div class="footer-text">
          <n-text depth="3" style="font-size: 12px;">
            Made with ❤️ by Baiyu
          </n-text>
        </div>
      </div>
    </n-layout-content>

    <!-- Create Modal -->
    <n-modal
      v-model:show="showCreateModal"
      title="新建 API 配置"
      preset="card"
      style="width: 500px"
      :mask-closable="false"
    >
      <n-form label-placement="left" label-width="100px">
        <n-form-item label="配置名称" required>
          <n-input 
            v-model:value="formData.name" 
            placeholder="例如：OpenAI 生产环境"
          />
        </n-form-item>

        <n-form-item label="服务商" required>
          <n-select
            :value="formData.provider"
            :options="providerOptions"
            @update:value="handleProviderChange"
            placeholder="选择服务商"
          />
        </n-form-item>

        <n-form-item label="Base URL" required>
          <n-input 
            v-model:value="formData.baseUrl" 
            placeholder="https://api.example.com/v1"
          />
          <template #feedback>
            <n-text depth="3" style="font-size: 12px;">
              已自动填入 {{ PRESET_PROVIDERS[formData.provider]?.name }} 默认地址，可手动修改
            </n-text>
          </template>
        </n-form-item>

        <n-form-item label="模型" required>
          <n-input 
            v-model:value="formData.model" 
            placeholder="例如：gpt-4o, claude-3-5-sonnet, qwen-max..."
          />
          <template #feedback>
            <n-text depth="3" style="font-size: 12px;">
              输入模型名称，可参考服务商官方文档
            </n-text>
          </template>
        </n-form-item>

        <n-form-item label="API Key" required>
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
          <n-button @click="showCreateModal = false">取消</n-button>
          <n-button type="primary" @click="handleCreate">创建</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- Edit Modal -->
    <n-modal
      v-model:show="showEditModal"
      title="编辑 API 配置"
      preset="card"
      style="width: 500px"
      :mask-closable="false"
    >
      <n-form label-placement="left" label-width="100px">
        <n-form-item label="配置名称" required>
          <n-input 
            v-model:value="formData.name" 
            placeholder="例如：OpenAI 生产环境"
          />
        </n-form-item>

        <n-form-item label="服务商" required>
          <n-select
            :value="formData.provider"
            :options="providerOptions"
            @update:value="handleProviderChange"
            placeholder="选择服务商"
          />
        </n-form-item>

        <n-form-item label="Base URL" required>
          <n-input 
            v-model:value="formData.baseUrl" 
            placeholder="https://api.example.com/v1"
          />
        </n-form-item>

        <n-form-item label="模型" required>
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
            placeholder="留空表示不修改"
          />
          <template #feedback>
            <n-text depth="3" style="font-size: 12px;">
              留空表示保持原 API Key 不变
            </n-text>
          </template>
        </n-form-item>
      </n-form>

      <template #footer>
        <n-space justify="end">
          <n-button @click="showEditModal = false">取消</n-button>
          <n-button type="primary" @click="handleUpdate">保存</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- Embedding Create Modal -->
    <n-modal
      v-model:show="showEmbeddingCreateModal"
      title="新建 Embedding API 配置"
      preset="card"
      style="width: 500px"
      :mask-closable="false"
    >
      <n-form label-placement="left" label-width="100px">
        <n-form-item label="配置名称" required>
          <n-input 
            v-model:value="embeddingFormData.name" 
            placeholder="例如：OpenAI Embedding"
          />
        </n-form-item>

        <n-form-item label="服务商" required>
          <n-select
            :value="embeddingFormData.provider"
            :options="providerOptions"
            @update:value="handleEmbeddingProviderChange"
            placeholder="选择服务商"
          />
        </n-form-item>

        <n-form-item label="Base URL" required>
          <n-input 
            v-model:value="embeddingFormData.baseUrl" 
            placeholder="https://api.openai.com/v1"
          />
          <template #feedback>
            <n-text depth="3" style="font-size: 12px;">
              已自动填入 {{ PRESET_PROVIDERS[embeddingFormData.provider]?.name }} 默认地址
            </n-text>
          </template>
        </n-form-item>

        <n-form-item label="Embedding 模型" required>
          <n-input 
            v-model:value="embeddingFormData.model" 
            placeholder="例如：text-embedding-3-small, embedding-2, bge-large-zh..."
          />
          <template #feedback>
            <n-text depth="3" style="font-size: 12px;">
              输入 Embedding 模型名称，可参考服务商官方文档
            </n-text>
          </template>
        </n-form-item>

        <n-form-item label="API Key" required>
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
          <n-button @click="showEmbeddingCreateModal = false">取消</n-button>
          <n-button type="primary" @click="handleEmbeddingCreate">创建</n-button>
        </n-space>
      </template>
    </n-modal>

    <!-- Embedding Edit Modal -->
    <n-modal
      v-model:show="showEmbeddingEditModal"
      title="编辑 Embedding API 配置"
      preset="card"
      style="width: 500px"
      :mask-closable="false"
    >
      <n-form label-placement="left" label-width="100px">
        <n-form-item label="配置名称" required>
          <n-input 
            v-model:value="embeddingFormData.name" 
            placeholder="例如：OpenAI Embedding"
          />
        </n-form-item>

        <n-form-item label="服务商" required>
          <n-select
            :value="embeddingFormData.provider"
            :options="providerOptions"
            @update:value="handleEmbeddingProviderChange"
            placeholder="选择服务商"
          />
        </n-form-item>

        <n-form-item label="Base URL" required>
          <n-input 
            v-model:value="embeddingFormData.baseUrl" 
            placeholder="https://api.openai.com/v1"
          />
        </n-form-item>

        <n-form-item label="Embedding 模型" required>
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
            <n-text depth="3" style="font-size: 12px;">
              留空表示保持原 API Key 不变
            </n-text>
          </template>
        </n-form-item>
      </n-form>

      <template #footer>
        <n-space justify="end">
          <n-button @click="showEmbeddingEditModal = false">取消</n-button>
          <n-button type="primary" @click="handleEmbeddingUpdate">保存</n-button>
        </n-space>
      </template>
    </n-modal>
  </n-layout>
</template>

<style scoped lang="scss">
.settings-view {
  height: 100%;
  background: var(--n-color);
}

.settings-content {
  height: 100%;
}

.settings-container {
  max-width: 700px;
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
  border-radius: 16px;
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

.settings-form {
  padding: 8px 0;
}

.about-content {
  padding: 8px 0;
}

.about-item {
  display: flex;
  align-items: center;
  padding: 12px 0;
  border-bottom: 1px solid var(--n-border-color);
}

.about-item:last-child {
  border-bottom: none;
}

.about-label {
  width: 100px;
  color: var(--n-text-color-3);
  font-size: 14px;
}

.about-link {
  color: var(--n-text-color-1);
  cursor: pointer;
  font-size: 14px;
}

.about-link:hover {
  color: var(--n-text-color-2);
}

.footer-text {
  text-align: center;
  margin-top: 40px;
  padding: 20px;
}
</style>
