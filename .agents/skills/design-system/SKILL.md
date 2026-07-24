---
name: design-system
description: BaiyuAISpace2's monochrome editorial design system (黑白编辑设计). MUST be loaded before adding or modifying any UI — new views, components, buttons, dialogs, toasts, animations, or styles in src/**/*.vue or src/styles. Use when asked to 改界面 / 加页面 / 调样式 / UI 排版, or any task that touches visual presentation.
---

2026-07-05 用户下达了前端全面重写指令（commit ef07363），确立了一套严格的
黑白编辑设计系统。之后所有 UI 改动必须遵守，**任何彩色、圆角、默认缓动
的出现都是回归 bug**。

## 权威源

Token 全部定义在 `src/styles/variables.scss`（含中文注释），写样式先读它、
引用它，不要硬编码字面值。要点速览：

- **配色**：纯黑白。`$bg` #FFFFFF、`$ink` #000000、`$ink-soft` #444444、
  `$ink-faint` #888888、`$surface` #F5F5F5、边框 `1px solid #000`。
  **不使用任何彩色**——成功/警告/错误也只用黑白灰 + 排版层次表达。
- **字体**：中文标题 `$font-serif`（Noto Serif SC，字重 700）；
  正文与英文 `$font-sans`（Inter，400–500）；代码 `$font-mono`。
- **圆角**：一律 0（直角）。`$radius-*` 全为 0，别绕过它。
- **动效**：只用 `$ease`（cubic-bezier(0.22,1,0.36,1)），时长 `$duration`
  0.5s / fast 0.3s / slow 0.9s。禁 linear 和 ease-in-out。

## variables.scss 之外的排版语言（来自用户原始规范）

- 大面积留白：区块 padding `$section-pad`（8rem）起步；
  非对称网格（1:1.2、1:1 交替）。
- 展示标题窄行高 `$leading-display`（1.15）+ 正文 `$leading-body`（1.65）。
- 区块前缀用 eyebrow label：`$label-size` 0.75rem、字间距 0.15em、全大写。
- 几何装饰：1px 黑色细线网格、嵌套方框（多层边框 opacity 0.4–0.8）、
  背景层 SVG 线框圆/线/矩形（opacity 0.04）、轨道圆点（8s 环绕）、
  旋转外框（60s linear infinite——装饰性旋转是唯一允许 linear 的地方）。
- 入场动画：opacity 0→1 + translateY(40px)→0 + scale(0.95)→1，
  滚动触发用 IntersectionObserver（threshold 0.15）。
- 悬浮反馈：translateY(-4px) + `$shadow-hover`（0 20px 60px rgba(0,0,0,0.08)）。
- 按钮交互：黑白反色（白底黑字 ↔ 黑底白字），过渡 0.5s。
- 氛围关键词：克制、优雅、精确、编辑设计、瑞士国际主义、黑白对比、技术人文。

## 交互约定（用户明确定过的规则）

- **所有提示、报错、警告统一到界面左下角**，弹窗形式（commit 5776a22）。
  新增任何 message/notification 都走这个统一机制，不要另起炉灶。
- **表单必须写中文 placeholder**：漏写会回落成 NaiveUI 的默认英文提示，
  这被用户当 bug 报过（commit 81eee83）。
- 提示文案说人话，不暴露内部术语（例：僵尸 Agent 的提示不能直接写"僵尸 Agent"）。

## 收尾

UI 改完用 `run-baiyuaispace2` skill 启动应用截图自查一遍排版，
再跑 `pnpm tauri build` 验证编译。
