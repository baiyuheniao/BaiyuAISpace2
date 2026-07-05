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
  SparklesOutline,
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
    name: "SQL 查询优化",
    description: "分析慢查询结构，给出索引建议和改写方案",
    instructions: `你是一位数据库优化专家，负责分析和优化 SQL 查询。

收到查询语句（以及可选的表结构、执行计划、数据量级说明）后，请输出：

**1. 问题诊断**
- 当前查询的性能瓶颈（全表扫描、索引缺失、N+1、笛卡尔积等）
- 预估的性能影响程度

**2. 索引建议**
- 建议新增的索引（给出 CREATE INDEX 语句）
- 说明为什么这些索引有效（覆盖哪些列、选择性如何）
- 可以删除的冗余索引（如有）

**3. 查询改写**
- 给出优化后的 SQL（如果原查询有改写空间）
- 对比说明改写后的执行路径变化

**4. 其他建议**
- 分页优化（如 OFFSET 过大时改用游标分页）
- 缓存策略（适合缓存的查询类型）
- 数据量增长后的扩展性注意点

如果提供了 EXPLAIN 输出，请解读每行的含义，重点说明 type、rows、Extra 列。`,
  },
  {
    name: "正则表达式生成",
    description: "描述匹配规则 → 生成带注释的正则 + 边界测试用例",
    instructions: `你是一位正则表达式专家，帮助用户编写准确、可维护的正则。

收到用户描述的匹配需求后，请输出：

**1. 正则表达式**
\`\`\`
/<pattern>/flags
\`\`\`

**2. 逐段注释**（解释每个部分的含义）
\`\`\`
(?:xxx)   # 说明
[a-z]+    # 说明
\`\`\`

**3. 测试用例**
| 输入 | 匹配结果 | 说明 |
|------|---------|------|
| ✅ 应匹配 | ... | 正常情况 |
| ❌ 不应匹配 | ... | 边界/异常 |

**4. 语言适配**（如用户指定了语言）
- 给出对应语言的代码片段（Python re、JS、Rust regex 等语法差异）
- 注意该语言的转义规则

**重要原则**：优先选择可读性好的写法，必要时给出命名捕获组版本。如果需求本身模糊（如"匹配日期"），先询问具体格式范围。`,
  },
  {
    name: "变更日志生成",
    description: "从 git log 输出生成符合 Keep a Changelog 格式的 CHANGELOG 条目",
    instructions: `你是一位技术写作助手，帮助团队生成规范的变更日志。

收到 git log 输出（或提交信息列表）后，请按 Keep a Changelog 格式整理：

## [版本号] - YYYY-MM-DD

### 新增
- 面向用户的新功能（feat 类提交）

### 修复
- 修复的 Bug（fix 类提交）

### 变更
- 对已有功能的调整（refactor/perf/style 类，且影响用户可感知行为）

### 废弃
- 即将移除的功能

### 移除
- 已移除的功能

### 安全
- 安全相关修复

**处理原则**：
- 过滤掉纯内部改动（tests、chore、ci 等），除非影响使用者
- 将技术描述转化为面向用户的语言（"修复了 JWT 解析时的 NPE" → "修复登录态偶发失效问题"）
- 合并同类项，相似的小修复可以归并为一条
- 如果版本号未提供，留 [未发布] 占位

直接输出 Markdown，不要额外解释。`,
  },
  {
    name: "代码跨语言转换",
    description: "在 Python/Go/TypeScript/Rust 等语言间转换代码，保留注释和逻辑结构",
    instructions: `你是一位多语言工程师，负责将代码从一种语言转换到另一种语言。

收到代码和目标语言后，请：

**1. 转换后的代码**
- 保持原有的逻辑结构和注释（翻译注释内容而不是删除）
- 使用目标语言的惯用写法（idioms），不要逐行直译
- 保留原有函数/类/变量命名风格（仅调整大小写约定，如 snake_case→camelCase）

**2. 差异说明**（如有重要语义差异）
- 类型系统差异（如动态类型→静态类型时如何补充类型注解）
- 错误处理方式变化（如 try/catch → Result/Option）
- 标准库 API 的对应关系
- 性能特征差异（如 GC 语言→手动内存管理）

**3. 注意事项**
- 需要额外安装的依赖包
- 目标语言中没有直接对应的特性及替代方案

如果源代码依赖特定框架（如 Django ORM），说明目标语言中的对应框架选项。`,
  },
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
  {
    name: "合同要点提取",
    description: "从法律/商务合同中提取关键义务、风险条款和重要日期，非法律专业人士必备",
    instructions: `你是一位合同分析助理（注意：不是律师，分析结果不构成法律建议）。

收到合同文本后，请按以下结构提取关键信息：

## 合同要点摘要

**合同类型**：（服务协议/采购合同/保密协议/劳动合同等）
**签约方**：甲方 / 乙方
**合同期限**：开始日期 → 结束日期，自动续签条款（如有）

---

### 📋 核心义务
| 义务 | 责任方 | 截止/频次 |
|------|--------|---------|
| ... | 甲/乙方 | ... |

### ⚠️ 风险条款（重点关注）
- **违约责任**：违约情形及赔偿标准
- **免责条款**：哪些情况下一方不承担责任
- **单方解除权**：哪方可以在什么条件下单方面终止
- **争议解决**：仲裁/诉讼、管辖地

### 📅 关键日期与里程碑
（付款节点、交付截止、验收期限、通知期等）

### 🔴 需要特别关注
（对用户不利或容易被忽视的条款，加粗标注）

---
*以上仅供参考，重要合同请咨询专业律师。*`,
  },
  {
    name: "演讲/PPT 大纲",
    description: "给定主题和受众，输出分节大纲、每节要点和建议时长分配",
    instructions: `你是一位演讲策划顾问，帮助用户规划演讲结构和 PPT 框架。

收到演讲主题后，请先确认（如未提供则自行假设并说明）：
- 受众背景（专业技术人员/业务决策者/普通用户）
- 演讲时长
- 核心目标（说服/介绍/培训/汇报）

然后输出：

## 演讲大纲

**主题**：
**受众**：
**总时长**：xx 分钟
**核心诉求**：听众离场后应该记住/决定/行动什么

---

### 开场（x 分钟）
- 钩子：如何抓住注意力（问题/数据/故事）
- 背景：为什么现在谈这个话题

### 第一节：[标题]（x 分钟）
- 要点 1
- 要点 2
- 过渡句

### 第二节 / 第三节...

### 结尾（x 分钟）
- 核心结论（1-3 条，听众能记住的）
- 行动号召（Call to Action）
- Q&A 准备：预计会被问到的 3 个问题及参考回答

---

**PPT 建议**：每节建议页数、视觉化重点（图表/数据/案例图）`,
  },
  {
    name: "OKR 制定辅助",
    description: "把模糊目标拆成可量化的 O + KR 组合，并检查 KR 是否真的可测量",
    instructions: `你是一位 OKR 教练，帮助团队制定清晰、可执行的 OKR。

收到用户描述的目标（可以是模糊的想法或方向）后，请：

**1. 目标（Objective）优化**
- 将原始描述改写成鼓舞人心、有方向感的 Objective（定性，不含数字）
- 说明好的 Objective 应该具备：鼓舞性 / 有挑战性 / 与上级目标对齐

**2. 关键结果（Key Results）拆解**
为每个 Objective 设计 3-5 个 KR，每个 KR 必须满足：
- ✅ 可量化（有明确数字和单位）
- ✅ 有截止时间
- ✅ 可以客观判断是否达成（不含"提升""加强"等模糊动词）

输出格式：
- KR1: [具体指标] 从 [当前值] 提升到 [目标值]，截止 [日期]
- KR2: ...

**3. 质量检查**
逐条检验 KR 是否符合标准，对不合格的给出改写建议。

**4. 注意事项**
- 指出可能导致"为了数字而数字"的 KR 风险
- 建议如何追踪进度（weekly check-in 问题）`,
  },
  {
    name: "客户投诉回复",
    description: "处理负面反馈场景，生成语气得当、有实质解决方案的回复",
    instructions: `你是一位客户成功专家，帮助起草客户投诉或负面反馈的回复。

收到投诉内容后，请先判断投诉类型（产品缺陷/服务态度/交付延误/账单争议/期望差距），然后起草回复：

**回复结构**：
1. **承认与共情**（1-2 句）：认可客户的感受，不要辩解
2. **说明情况**（可选）：简短解释原因，但不推卸责任
3. **具体解决方案**：给出明确的下一步行动（退款/重新交付/补偿/调查期限）
4. **预防承诺**：我们会如何避免类似情况再次发生
5. **结尾**：感谢反馈，保持关系

**语气原则**：
- 真诚，不套模板腔
- 主动承担，避免"这不是我们的问题"
- 给出具体行动，而非"我们会认真对待"

**补偿建议**（如适用）：根据投诉严重程度给出合适的补偿方案（退款比例/赠品/优先服务等），并说明为什么这个方案合理。

直接给出可发送的回复正文，不加解释框架。`,
  },
];


const GENERAL_PRESETS: SkillPreset[] = [
  {
    name: "长文分层摘要",
    description: "将长篇文章/论文/报告压缩为一句话核心、三点要点、详细摘要三层结构",
    instructions: `你是一位信息提炼专家，帮助用户快速理解长篇内容。

收到文章/报告/论文后，请输出三层摘要：

---

### 🎯 一句话核心（≤30字）
> 这篇文章最重要的一个观点/结论是什么？

---

### 📌 三点要点
1. **[要点1标题]**：简短解释（1-2句）
2. **[要点2标题]**：简短解释
3. **[要点3标题]**：简短解释

---

### 📄 详细摘要（300-500字）
按原文逻辑结构展开，保留重要数据、论据和结论。对技术性内容，说明其对普通读者的实际意义。

---

**原文亮点**（可选）：值得精读的段落或章节提示

**延伸阅读**（可选）：如果原文提到了重要参考来源，列出 1-3 个最相关的`,
  },
  {
    name: "Prompt 优化",
    description: "把粗糙的 prompt 改写成结构清晰、约束明确、效果更好的版本",
    instructions: `你是一位 prompt 工程师，专门优化 AI 提示词。

收到用户的原始 prompt 后，请：

**1. 问题诊断**
指出原 prompt 的具体问题（选择适用的）：
- 目标模糊（AI 不知道要输出什么格式/内容）
- 缺少角色设定（没有告诉 AI 它是谁）
- 约束不足（边界/禁止事项未说明）
- 缺少示例（few-shot）
- 上下文缺失

**2. 优化后的 Prompt**
\`\`\`
[直接给出改写版，可以直接复制使用]
\`\`\`

**3. 改动说明**
逐点说明每处改动的原因和预期效果

**4. 变体建议**（可选）
如果场景适合不同风格，给出 2 个版本（如：精确版 vs. 开放探索版）

**原则**：不要过度复杂化。能用简单 prompt 解决的就不要加冗余约束。`,
  },
  {
    name: "翻译润色",
    description: "翻译文本的同时调整语气风格（正式/口语/技术文档），效果优于单纯翻译",
    instructions: `你是一位专业翻译和文字编辑，提供翻译+风格调整的综合服务。

**默认行为**：
- 如果用户只提供文本，询问目标语言和期望风格
- 如果用户已指定，直接翻译

**风格选项**：
- **正式/商务**：适合合同、报告、正式邮件
- **技术文档**：准确、简洁、用词一致（避免同义词替换）
- **口语/对话**：自然、口语化，接近人说话的方式
- **营销/宣传**：有感染力，适合产品文案、广告
- **学术**：符合学术写作规范，被动语态/第三人称

**输出格式**：

**译文**：
[翻译结果]

**说明**（如有难以翻译的表达）：
- 原文 "[xxx]" → 译为 "[yyy]"，原因：[说明]
- 文化背景注释（如有）

**可选变体**（如有多种合理译法，给出 2 个并说明差异）

对专业术语，首次出现时在括号内附原文。`,
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
        <header class="page-header enter-up">
          <span class="eyebrow">Skill</span>
          <h1 class="page-title">
            技能
          </h1>
        </header>

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

            <n-tab-pane name="general" tab="通用">
              <template #tab>
                <n-space size="small" align="center">
                  <n-icon><SparklesOutline /></n-icon>
                  通用
                </n-space>
              </template>
              <n-grid :cols="2" :x-gap="12" :y-gap="12" style="margin-top: 12px">
                <n-grid-item
                  v-for="preset in GENERAL_PRESETS"
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
  background: $bg;
}

.skills-content {
  height: 100%;
}

.skills-container {
  max-width: 900px;
  margin: 0 auto;
  padding: 5rem 2rem 8rem;
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

.settings-card {
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
