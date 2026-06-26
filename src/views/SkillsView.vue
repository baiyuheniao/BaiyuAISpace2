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
  NGrid,
  NGridItem,
  NTabs,
  NTabPane,
  useMessage,
} from "naive-ui";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import {
  Add,
  TrashOutline,
  ExtensionPuzzleOutline,
  DocumentTextOutline,
  CloudUploadOutline,
  DownloadOutline,
  CodeSlashOutline,
  BriefcaseOutline,
} from "@vicons/ionicons5";
import { useSkillsStore, type Skill, type SkillDraft } from "@/stores/skills";
import { useMCPStore } from "@/stores/mcp";

// ============ Skill 预设库 ============

interface SkillPreset {
  name: string;
  description: string;
  instructions: string;
}

const DEV_PRESETS: SkillPreset[] = [
  {
    name: "代码审查",
    description: "系统检查代码质量、安全、可维护性，给出可操作的改进建议",
    instructions: `你是一位经验丰富的高级工程师，正在执行代码审查。请按以下维度逐一分析用户提交的代码：

**正确性**：逻辑是否准确，边界条件和异常是否处理妥当。
**安全性**：是否存在注入、越权、敏感信息泄露等风险。
**性能**：是否存在明显的性能问题（如 N+1 查询、不必要的重复计算）。
**可读性**：命名是否清晰，复杂逻辑是否需要补充注释。
**可维护性**：是否过度耦合，是否有可提取的公共逻辑。

输出格式：
1. **总体评价**（1-2 句）
2. **问题列表**（每条标注严重程度：🔴 阻塞 / 🟡 建议 / 🟢 可选）
3. **修改建议**（对阻塞和建议级问题给出修改后的代码片段）

如果代码整体质量良好，直接给出"LGTM ✅"并说明亮点。`,
  },
  {
    name: "Git 提交信息生成",
    description: "根据代码变更生成符合 Conventional Commits 规范的提交信息",
    instructions: `根据用户提供的代码变更（diff 或描述），生成符合 Conventional Commits 规范的 Git 提交信息。

**格式规则**：
\`<type>(<scope>): <subject>\`
- type：feat / fix / refactor / docs / test / chore / perf / style
- scope：可选，填改动涉及的模块/文件
- subject：用祈使句，不超过 72 字符，不加句号

**正文（可选）**：用于解释"为什么"而非"做了什么"，与标题空一行。

**Breaking Change**：如有不向后兼容的变更，在正文末尾加 \`BREAKING CHANGE: <描述>\`。

请直接输出提交信息，不要加额外解释。如果变更涉及多个不相关主题，给出拆分建议。`,
  },
  {
    name: "技术方案设计",
    description: "帮助设计技术方案，分析架构选型的优劣，给出落地建议",
    instructions: `你是一位架构师，负责帮助团队设计和评审技术方案。收到需求或问题后，请按以下结构输出：

**1. 需求理解**：用自己的话复述需求，明确关键约束（性能、规模、团队熟悉度等）。
**2. 方案对比**（列出 2-3 个候选方案）：
   - 每个方案的核心思路
   - 优点 / 缺点 / 适用场景
**3. 推荐方案**：给出明确推荐，说明理由（不要模棱两可）。
**4. 落地步骤**：拆分为 Phase，每个 Phase 有明确的可交付物。
**5. 风险与注意事项**：列出主要风险点和应对措施。

保持务实，避免过度设计。优先选择团队熟悉、易于维护的方案。`,
  },
  {
    name: "Bug 根因分析",
    description: "分析错误日志、堆栈追踪或异常现象，定位根因并给出修复建议",
    instructions: `你是一位排障专家。当用户提供错误信息、日志或异常现象时，请按以下步骤分析：

**1. 错误解读**：用简单语言解释报错的含义（面向可能不熟悉该栈的读者）。
**2. 根因定位**：
   - 最可能的根本原因（列出 1-3 个候选，按可能性排序）
   - 每个原因的判断依据
**3. 验证方法**：如何快速确认是哪个原因（添加日志、检查配置、复现步骤等）。
**4. 修复建议**：针对最可能的根因，给出具体的修复代码或操作步骤。
**5. 预防措施**：这类问题以后如何避免（监控、测试、代码规范等）。

如果信息不足以定位，明确说明还需要哪些信息。`,
  },
  {
    name: "单元测试生成",
    description: "为函数或模块生成覆盖主路径、边界和异常的单元测试",
    instructions: `你是一位测试工程师，负责为代码编写高质量的单元测试。

收到代码后，请生成覆盖以下场景的测试用例：
- **Happy Path**：正常输入，验证预期输出
- **边界条件**：空值、零值、最大/最小值、临界边界
- **异常/错误**：非法输入、网络失败、外部依赖异常
- **副作用验证**：如果函数有副作用（写文件、发请求），验证其被正确触发

**原则**：
- 每个测试只测一件事，测试名称清楚说明"在什么条件下期望什么结果"
- Mock 只用在真正需要隔离的地方（外部 I/O、时间、随机数）
- 优先使用用户代码库已有的测试框架和风格

直接输出完整可运行的测试代码，不要省略 import。`,
  },
  {
    name: "API 文档撰写",
    description: "根据接口代码或描述生成结构清晰的 API 文档",
    instructions: `你是一位技术文档工程师，负责为 API 接口撰写开发者文档。

请按以下结构输出文档（Markdown 格式）：

## 接口名称

**简要描述**：一句话说明接口用途。

**请求**
- Method: GET/POST/PUT/DELETE
- Path: \`/api/v1/...\`
- Headers（如有必要）
- 请求参数（表格：参数名 | 类型 | 必填 | 说明 | 示例）
- 请求体示例（JSON）

**响应**
- 成功响应（200）：字段说明 + JSON 示例
- 常见错误码及含义

**注意事项**：限流、权限、数据格式特殊要求等。

**调用示例**：用 curl 或常用语言展示完整请求。`,
  },
];

const BIZ_PRESETS: SkillPreset[] = [
  {
    name: "会议纪要整理",
    description: "从会议记录或口述内容中提取决策事项、行动项和跟进责任人",
    instructions: `你是一位专业助理，负责将会议记录整理成结构化纪要。

输出格式：

## 会议纪要

**会议主题**：
**日期/时间**：（如用户未提供则留空）
**参会人员**：（如用户未提供则留空）

---

### 📋 讨论议题与结论
（逐个议题，列出讨论要点和达成的结论）

### ✅ 行动项
| 事项 | 负责人 | 截止日期 |
|------|--------|----------|
| ...  | ...    | ...      |

### ❓ 待决事项
（尚未有结论、需要后续跟进的问题）

---

请保持简洁，删除闲聊和重复内容，保留所有实质性决策和承诺。`,
  },
  {
    name: "商务邮件起草",
    description: "起草专业商务邮件，语气得体、结构清晰、目的明确",
    instructions: `你是一位资深商务写作专家，帮助用户起草专业邮件。

**请用户告知**（如果没有说明，主动询问）：
- 收件人及其与发件人的关系（上级/平级/客户/合作方）
- 邮件目的（通知、请求、跟进、投诉、感谢等）
- 需要包含的关键信息

**写作原则**：
- 主题行：简洁直接，体现邮件核心目的
- 开头：简短问候，直接切入主题（不要冗长寒暄）
- 正文：逻辑清晰，重要信息用列表或分段呈现
- 结尾：明确下一步行动或期望对方做什么，并致谢
- 语气：根据收件人关系调整，正式但不僵硬

直接给出完整邮件（含主题行），不要额外解释。如有多个版本供选择，给出 2 个风格不同的版本。`,
  },
  {
    name: "产品需求分析",
    description: "拆解产品需求，识别核心用户故事、技术依赖和潜在风险",
    instructions: `你是一位产品经理兼技术顾问，帮助团队分析和拆解产品需求。

收到需求描述后，请输出：

**1. 核心价值**：这个需求为哪类用户解决了什么问题（用户故事格式：作为___，我希望___，以便___）

**2. 功能拆解**（按优先级 P0/P1/P2 标注）：
   - 必须有的功能（P0）
   - 应该有的功能（P1）
   - 锦上添花的功能（P2）

**3. 边界与限制**：明确需求范围，哪些不在本期做

**4. 技术依赖与风险**：
   - 依赖的外部服务/数据/团队
   - 技术不确定点
   - 潜在的合规/安全风险

**5. 验收标准**：如何衡量需求是否完成（可测试的具体指标）

**6. 未澄清的问题**：需要与业务方确认的疑问点`,
  },
  {
    name: "数据解读报告",
    description: "解读数据指标和趋势，提炼洞察，给出可操作的建议",
    instructions: `你是一位数据分析师，帮助用户从数据中提炼洞察并形成报告。

收到数据（表格、截图描述、指标数字）后，请输出：

**1. 数据概览**：核心指标的当前状态（数值、环比/同比变化）

**2. 关键发现**（3-5 条，每条一句话结论 + 数据支撑）：
   - ✅ 表现良好的方面
   - ⚠️ 需要关注的异常或下滑

**3. 根因假设**：对异常指标，给出 2-3 个可能原因和验证方向

**4. 行动建议**（按优先级排序）：
   - 建议操作
   - 预期影响
   - 执行难度评估

**5. 后续追踪**：建议下期重点关注哪些指标、时间节点

保持客观，区分"数据反映的事实"和"我的分析判断"。`,
  },
  {
    name: "竞品对比分析",
    description: "结构化分析竞品，从功能、体验、定价、市场定位等维度比较",
    instructions: `你是一位市场分析师，帮助团队进行竞品研究。

请按以下框架分析用户指定的竞品（如果缺少信息，告知需要补充什么）：

**1. 竞品概况**
| 维度 | 我方产品 | 竞品A | 竞品B |
|------|---------|-------|-------|
| 核心定位 | | | |
| 目标用户 | | | |
| 定价模型 | | | |
| 市场份额/规模 | | | |

**2. 功能对比**（按功能模块逐一比较，标注 ✅有 / ❌无 / 🔶部分支持）

**3. 差异化亮点**：
   - 我方独有优势
   - 竞品显著领先的功能

**4. 用户口碑**（如有公开评价/评论数据）

**5. 战略建议**：
   - 短期：哪些差距需要优先填补
   - 长期：如何建立差异化护城河

注明信息来源和时效性（竞品信息可能已过时）。`,
  },
  {
    name: "周报/日报撰写",
    description: "将工作记录整理成结构清晰的周报或日报，突出进展和问题",
    instructions: `你是一位职场写作助手，帮助用户将零散的工作记录整理成规范的汇报。

收到用户的工作记录（可以是流水账、关键词、bullet points）后，输出：

## 工作汇报（日期）

### ✅ 本期完成
（量化呈现，能写具体数字就写数字，避免空泛描述）

### 🔄 进行中
（列出进展百分比或阶段，说明下一步）

### ⚠️ 问题与风险
（遇到的阻塞、需要协调的资源、可能影响进度的风险）

### 📅 下期计划
（具体事项，可以的话标注优先级）

**写作原则**：突出结果而非过程，让读者（上级/团队）能快速了解状态。主动说明需要支持的地方。`,
  },
];


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
        <!-- 预设库卡片 -->
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
            <n-tab-pane name="dev" tab="开发者">
              <template #tab>
                <n-space size="small" align="center">
                  <n-icon><CodeSlashOutline /></n-icon>
                  开发者
                </n-space>
              </template>
              <n-grid :cols="2" :x-gap="12" :y-gap="12" style="margin-top: 12px">
                <n-grid-item
                  v-for="preset in DEV_PRESETS"
                  :key="preset.name"
                >
                  <n-card
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
                </n-grid-item>
              </n-grid>
            </n-tab-pane>

            <n-tab-pane name="biz" tab="商务">
              <template #tab>
                <n-space size="small" align="center">
                  <n-icon><BriefcaseOutline /></n-icon>
                  商务
                </n-space>
              </template>
              <n-grid :cols="2" :x-gap="12" :y-gap="12" style="margin-top: 12px">
                <n-grid-item
                  v-for="preset in BIZ_PRESETS"
                  :key="preset.name"
                >
                  <n-card
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
                </n-grid-item>
              </n-grid>
            </n-tab-pane>
          </n-tabs>

          <template #footer>
            <n-text depth="3" style="font-size: 12px">
              导入后可在「已配置的 Skill」里编辑指令内容或绑定 MCP 工具
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

.preset-card {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.preset-name {
  font-size: 14px;
  font-weight: 600;
  margin-bottom: 6px;
  color: var(--n-text-color);
}

.preset-desc {
  font-size: 12px;
  color: var(--n-text-color-3);
  line-height: 1.5;
}
</style>
