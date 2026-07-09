<!-- 感谢贡献！提交前请先阅读 CONTRIBUTING.md -->

## 变更说明

<!-- 做了什么、为什么。关联 Issue 请写 Closes #编号 -->

## 变更类型

- [ ] fix（修复 Bug）
- [ ] feat（新功能）
- [ ] docs / chore / refactor（文档、杂项、重构）

## 自查清单

- [ ] `pnpm build` 通过（含 `vue-tsc --noEmit` 类型检查）
- [ ] `cargo build`（在 `src-tauri/` 下）通过
- [ ] 新增的 `.rs` / `.ts` / `.vue` 文件包含 MPL-2.0 头注释
- [ ] UI 改动符合黑白编辑设计系统（无彩色、无圆角；token 见 `src/styles/variables.scss`），表单有中文 placeholder
- [ ] 网络请求未对流式响应 / 大文件下载设置总超时（只允许读间隔超时）
- [ ] 提交信息格式为 `type: 中文摘要`（如 `fix: 修复知识库导入进度不更新`）
