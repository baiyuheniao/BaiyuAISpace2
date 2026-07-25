<!-- This Source Code Form is subject to the terms of the Mozilla Public
  - License, v. 2.0. If a copy of the MPL was not distributed with this
  - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<script setup lang="ts">
import { computed, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useMessage } from "@/composables/useNotify";

interface ImportedMcpPreview {
  name: string;
  serverType: string;
  command: string;
  args: string[];
  url?: string;
  environmentVariableNames: string[];
  credentialFieldNames: string[];
  warnings: string[];
}

interface ImportedSkillPreview {
  name: string;
  description: string;
  resourceFileNames: string[];
}

interface AgentImportPreview {
  sourcePath: string;
  sourceKind: string;
  mcpServers: ImportedMcpPreview[];
  skill?: ImportedSkillPreview;
  warnings: string[];
}

interface ImportResult {
  importedNames: string[];
  warnings: string[];
}

interface ProjectRuleFile {
  path: string;
  filename: string;
  content: string;
}

type Section = "mcp" | "skill" | "rules";

const message = useMessage();
const section = ref<Section>("mcp");
const isBusy = ref(false);

const mcpPreview = ref<AgentImportPreview | null>(null);
const selectedMcpNames = ref<string[]>([]);
const skillPreview = ref<AgentImportPreview | null>(null);
const projectRules = ref<ProjectRuleFile[]>([]);
const selectedRulePath = ref<string | null>(null);
const ruleContent = ref("");
const selectedRule = computed(() => projectRules.value.find((rule) => rule.path === selectedRulePath.value) ?? null);

const getOnePath = (result: string | string[] | null): string | null =>
  Array.isArray(result) ? result[0] ?? null : result;

const preview = async (path: string): Promise<AgentImportPreview> =>
  invoke<AgentImportPreview>("preview_agent_import", { sourcePath: path });

const chooseMcpConfig = async () => {
  const path = getOnePath(await openFileDialog({
    multiple: false,
    filters: [{ name: "Agent 配置", extensions: ["json", "toml"] }],
  }));
  if (!path) return;
  isBusy.value = true;
  try {
    const result = await preview(path);
    if (result.mcpServers.length === 0) throw new Error("这个文件中没有可导入的 MCP 服务");
    mcpPreview.value = result;
    selectedMcpNames.value = result.mcpServers.map((server) => server.name);
  } catch (error) {
    message.error(String(error));
  } finally {
    isBusy.value = false;
  }
};

const importMcps = async () => {
  if (!mcpPreview.value || selectedMcpNames.value.length === 0) return;
  isBusy.value = true;
  try {
    const result = await invoke<ImportResult>("import_mcp_servers", {
      sourcePath: mcpPreview.value.sourcePath,
      selectedNames: selectedMcpNames.value,
    });
    message.success(`已导入 ${result.importedNames.length} 个 MCP 服务，均处于关闭状态`);
    result.warnings.forEach((warning) => message.warning(warning));
    mcpPreview.value = null;
    selectedMcpNames.value = [];
  } catch (error) {
    message.error(String(error));
  } finally {
    isBusy.value = false;
  }
};

const chooseSkillDirectory = async () => {
  const path = getOnePath(await openFileDialog({ multiple: false, directory: true }));
  if (!path) return;
  isBusy.value = true;
  try {
    const result = await preview(path);
    if (!result.skill) throw new Error("所选文件夹中没有可导入的 SKILL.md");
    skillPreview.value = result;
  } catch (error) {
    message.error(String(error));
  } finally {
    isBusy.value = false;
  }
};

const importSkill = async () => {
  if (!skillPreview.value) return;
  isBusy.value = true;
  try {
    const result = await invoke<ImportResult>("import_skill_directory", { sourcePath: skillPreview.value.sourcePath });
    message.success(`已导入 Skill「${result.importedNames[0]}」，请检查后启用`);
    skillPreview.value = null;
  } catch (error) {
    message.error(String(error));
  } finally {
    isBusy.value = false;
  }
};

const chooseProjectDirectory = async () => {
  const path = getOnePath(await openFileDialog({ multiple: false, directory: true }));
  if (!path) return;
  isBusy.value = true;
  try {
    const rules = await invoke<ProjectRuleFile[]>("scan_project_rule_files", { rootPath: path });
    projectRules.value = rules;
    if (rules.length === 0) {
      selectedRulePath.value = null;
      ruleContent.value = "";
      message.info("这个项目中没有发现 CLAUDE.md 或 AGENTS.md");
      return;
    }
    selectRule(rules[0]);
    message.success(`发现 ${rules.length} 份项目规则`);
  } catch (error) {
    message.error(String(error));
  } finally {
    isBusy.value = false;
  }
};

const selectRule = (rule: ProjectRuleFile) => {
  selectedRulePath.value = rule.path;
  ruleContent.value = rule.content;
};

const saveRule = async () => {
  if (!selectedRule.value) return;
  isBusy.value = true;
  try {
    await invoke("save_project_rule_file", { path: selectedRule.value.path, content: ruleContent.value });
    selectedRule.value.content = ruleContent.value;
    message.success(`${selectedRule.value.filename} 已保存到原项目`);
  } catch (error) {
    message.error(String(error));
  } finally {
    isBusy.value = false;
  }
};

const importRuleAsSkill = async () => {
  if (!selectedRule.value) return;
  isBusy.value = true;
  try {
    const result = await invoke<ImportResult>("import_project_rule_as_skill", { path: selectedRule.value.path });
    message.success(`已创建 Skill 草稿「${result.importedNames[0]}」`);
  } catch (error) {
    message.error(String(error));
  } finally {
    isBusy.value = false;
  }
};
</script>

<template>
  <div class="migration-page">
    <header class="page-head enter-up">
      <div>
        <p class="eyebrow">Import</p>
        <h1>迁移中心</h1>
      </div>
      <p class="head-note">迁移你在其他Agent软件中的配置</p>
    </header>

    <nav class="migration-nav enter-up" aria-label="迁移类型">
      <button :class="{ active: section === 'mcp' }" @click="section = 'mcp'">01 · MCP 服务</button>
      <button :class="{ active: section === 'skill' }" @click="section = 'skill'">02 · Skill 技能</button>
      <button :class="{ active: section === 'rules' }" @click="section = 'rules'">03 · 项目规则</button>
    </nav>

    <section :key="section" class="migration-section">
      <template v-if="section === 'mcp'">
      <div class="intro-grid">
        <div>
          <p class="section-label">MCP IMPORT</p>
          <h2>在这里迁移MCP服务</h2>
        </div>
        <div class="rule-copy">
          支持 Claude Desktop、Claude Code 的 JSON 配置与 Codex TOML 配置。环境变量、请求头和认证信息不会被复制；导入后的服务默认关闭。
        </div>
      </div>
      <button class="primary-action" :disabled="isBusy" @click="chooseMcpConfig">选择 MCP 配置文件</button>

      <div v-if="mcpPreview" class="preview-block enter-up">
        <div class="preview-head">
          <span>解析来源：{{ mcpPreview.sourceKind }}</span>
          <span>{{ mcpPreview.mcpServers.length }} 个可迁移服务</span>
        </div>
        <label v-for="server in mcpPreview.mcpServers" :key="server.name" class="mcp-row">
          <input v-model="selectedMcpNames" type="checkbox" :value="server.name">
          <span class="server-name">{{ server.name }}</span>
          <span class="server-detail">{{ server.serverType === 'stdio' ? `${server.command} ${server.args.join(' ')}` : server.url }}</span>
          <span v-if="server.environmentVariableNames.length || server.credentialFieldNames.length" class="security-mark">需补充凭证</span>
        </label>
        <p v-for="warning in mcpPreview.warnings" :key="warning" class="warning">注：{{ warning }}</p>
        <div class="action-line"><button class="primary-action" :disabled="isBusy || selectedMcpNames.length === 0" @click="importMcps">导入已选服务</button></div>
      </div>
      </template>

      <template v-else-if="section === 'skill'">
      <div class="intro-grid">
        <div>
          <p class="section-label">SKILL IMPORT</p>
          <h2>在这里迁移Skill技能</h2>
        </div>
        <div class="rule-copy">选择包含 <code>SKILL.md</code> 的文件夹。正文会成为 Skill 指令，同级常规文件会作为资源副本保存；导入的 Skill 默认关闭。</div>
      </div>
      <button class="primary-action" :disabled="isBusy" @click="chooseSkillDirectory">选择 Skill 文件夹</button>
      <div v-if="skillPreview?.skill" class="preview-block skill-preview enter-up">
        <p class="section-label">准备导入</p>
        <h3>{{ skillPreview.skill.name }}</h3>
        <p>{{ skillPreview.skill.description }}</p>
        <p class="resource-note">资源文件：{{ skillPreview.skill.resourceFileNames.length ? skillPreview.skill.resourceFileNames.join(' · ') : '无' }}</p>
        <div class="action-line"><button class="primary-action" :disabled="isBusy" @click="importSkill">导入为关闭状态的 Skill</button></div>
      </div>
      </template>

      <template v-else>
      <div class="intro-grid">
        <div>
          <p class="section-label">RULES IMPORT</p>
          <h2>在这里维护项目规则，<br>也能在这里导入。</h2>
        </div>
        <div class="rule-copy"><code>CLAUDE.md</code> 和 <code>AGENTS.md</code> 是 Agent 的项目说明书。选择项目后可发现、编辑并保存回原文件；也可创建一份关闭状态的 Skill 草稿。</div>
      </div>
      <button class="primary-action" :disabled="isBusy" @click="chooseProjectDirectory">选择项目文件夹</button>
      <div v-if="projectRules.length" class="rule-workbench enter-up">
        <aside class="rule-list">
          <button v-for="rule in projectRules" :key="rule.path" :class="{ active: selectedRulePath === rule.path }" @click="selectRule(rule)">
            <span>{{ rule.filename }}</span><small>{{ rule.path }}</small>
          </button>
        </aside>
        <div class="editor-pane">
          <div class="editor-head"><span>{{ selectedRule?.filename }}</span><span>保存位置：原项目</span></div>
          <textarea v-model="ruleContent" :placeholder="'在这里编辑项目规则…'" spellcheck="false" />
          <div class="action-line">
            <button class="secondary-action" :disabled="isBusy" @click="importRuleAsSkill">导入为 Skill 草稿</button>
            <button class="primary-action" :disabled="isBusy" @click="saveRule">保存原文件</button>
          </div>
        </div>
      </div>
      <div v-else class="empty-state enter-up">尚未选择项目文件夹；扫描只会读取 <code>CLAUDE.md</code> 与 <code>AGENTS.md</code>，并跳过 <code>.git</code>、<code>node_modules</code>、构建目录。</div>
      </template>
    </section>
  </div>
</template>

<style scoped lang="scss">
.migration-page { height: 100%; overflow-y: auto; padding: 4rem 5vw 7rem; color: $ink; background: $bg; }
.page-head { display: flex; justify-content: space-between; gap: 3rem; align-items: end; padding-bottom: 2.5rem; border-bottom: $border; }
.eyebrow, .section-label { margin: 0 0 .65rem; font-size: $label-size; letter-spacing: $label-tracking; text-transform: uppercase; color: $ink-faint; }
h1, h2, h3 { margin: 0; font-family: $font-serif; font-weight: 700; line-height: $leading-display; }
h1 { font-size: 2.5rem; }
h2 { font-size: clamp(1.7rem, 3vw, 3.2rem); }
h3 { font-size: 1.5rem; }
.head-note, .rule-copy { max-width: 31rem; margin: 0; color: $ink-soft; line-height: $leading-body; }
.migration-nav { display: grid; grid-template-columns: repeat(3, 1fr); border-bottom: $border; animation-delay: .12s; }
.migration-nav button, .rule-list button { border: 0; background: transparent; color: $ink-soft; text-align: left; cursor: pointer; font-family: $font-serif; }
.migration-nav button { padding: 1rem 1.25rem; border-right: $border; transition: background $duration $ease, color $duration $ease; }
.migration-nav button:last-child { border-right: 0; }
.migration-nav button:hover, .migration-nav button.active { background: $ink; color: $bg; }
.migration-section { padding-top: 4.5rem; max-width: 1100px; animation: migration-section-enter $duration-slow $ease both; }
.intro-grid { display: grid; grid-template-columns: minmax(0, 1.25fr) minmax(250px, .75fr); gap: 4rem; align-items: end; margin-bottom: 2.5rem; }
code { font-family: $font-mono; font-size: .9em; }
.primary-action, .secondary-action { border: $border; border-radius: $radius-sm; padding: .85rem 1.1rem; cursor: pointer; font-family: $font-sans; font-weight: 600; transition: background $duration $ease, color $duration $ease; }
.primary-action { background: $ink; color: $bg; }
.secondary-action { background: $bg; color: $ink; }
.primary-action:hover:not(:disabled), .secondary-action:hover:not(:disabled) { background: $surface; color: $ink; }
button:disabled { cursor: not-allowed; opacity: .45; }
.preview-block { margin-top: 3rem; border: $border; }
.preview-head, .editor-head { display: flex; justify-content: space-between; gap: 1rem; padding: .8rem 1rem; border-bottom: $border; color: $ink-soft; font-family: $font-mono; font-size: .73rem; }
.mcp-row { display: grid; grid-template-columns: auto minmax(130px, .3fr) 1fr auto; gap: 1rem; align-items: center; padding: 1rem; border-bottom: $border-faint; cursor: pointer; }
.mcp-row input { accent-color: $ink; }
.server-name { font-family: $font-serif; font-weight: 700; }
.server-detail { min-width: 0; color: $ink-soft; font-family: $font-mono; font-size: .75rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.security-mark { border: $border-faint; padding: .2rem .4rem; font-size: .7rem; color: $ink-soft; }
.warning, .resource-note { margin: 1rem; color: $ink-soft; font-size: .85rem; line-height: $leading-body; }
.action-line { display: flex; justify-content: flex-end; gap: .75rem; padding: 1rem; border-top: $border-faint; }
.skill-preview { padding-top: 1.5rem; }
.skill-preview > :not(.action-line) { margin-left: 1.25rem; margin-right: 1.25rem; }
.rule-workbench { display: grid; grid-template-columns: minmax(220px, .32fr) minmax(0, 1fr); margin-top: 3rem; border: $border; min-height: 510px; }
.rule-list { border-right: $border; overflow-y: auto; }
.rule-list button { display: flex; flex-direction: column; gap: .4rem; width: 100%; padding: 1rem; border-bottom: $border-faint; transition: background $duration $ease, color $duration $ease; }
.rule-list button.active, .rule-list button:hover { background: $ink; color: $bg; }
.rule-list small { color: inherit; opacity: .6; font-family: $font-mono; font-size: .65rem; overflow: hidden; text-overflow: ellipsis; }
.editor-pane { display: flex; flex-direction: column; min-width: 0; }
.editor-pane textarea { flex: 1; min-height: 380px; width: 100%; resize: vertical; border: 0; outline: 0; padding: 1.25rem; color: $ink; background: $bg; font: .84rem/1.7 $font-mono; }
.empty-state { margin-top: 3rem; padding: 2rem; border: $border; color: $ink-soft; line-height: $leading-body; }
@keyframes migration-section-enter {
  from { opacity: 0; transform: translateY(40px) scale(.95); }
  to { opacity: 1; transform: translateY(0) scale(1); }
}
@media (max-width: 820px) { .migration-page { padding: 2rem 1.5rem 4rem; } .page-head, .intro-grid { grid-template-columns: 1fr; display: grid; gap: 1.5rem; align-items: start; } .migration-nav { grid-template-columns: 1fr; } .migration-nav button { border-right: 0; border-bottom: $border-faint; } .mcp-row { grid-template-columns: auto 1fr; } .server-detail { grid-column: 2; } .security-mark { grid-column: 2; justify-self: start; } .rule-workbench { grid-template-columns: 1fr; } .rule-list { border-right: 0; border-bottom: $border; max-height: 180px; } }
</style>
