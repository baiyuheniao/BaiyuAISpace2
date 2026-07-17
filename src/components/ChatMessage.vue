<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  ChatMessage.vue - 聊天消息组件
  
  功能说明:
  - 显示单条聊天消息 (用户/AI)
  - Markdown 内容渲染 (代码高亮)
  - 消息时间显示
  - 复制消息内容
  - 流式输出动画
  
  组成部分:
  - 消息头像
  - 消息头部 (作者 + 时间)
  - 消息内容 (Markdown 渲染)
  - 流式输出指示器
  - 错误提示
  - 操作按钮 (复制)
-->

<script setup lang="ts">
// 导入 Vue 相关功能
import { computed, ref, watch, onMounted, onBeforeUnmount, nextTick } from "vue";

// 导入 NaiveUI 组件
import { NAvatar, NIcon, NSpin, NAlert, NTooltip } from "naive-ui";

// Markdown 渲染管线。marked/KaTeX/DOMPurify/hljs/Mermaid 的全局配置都在该
// 模块顶层完成，整个应用只执行一次——不能放回本组件的 <script setup>：
// setup 顶层语句每挂载一条消息都会重新执行，会给全局单例反复叠加扩展/钩子，
// 长会话下渲染越来越慢（详见 utils/markdown.ts 头部说明）。
import { renderMarkdown, renderMermaidDiagrams } from "@/utils/markdown";

// 导入消息类型
import type { Message } from "@/stores/chat";

// 导入图标
import { Person, Sparkles, Copy } from "@vicons/ionicons5";

// ============ Props 定义 ============

const props = defineProps<{
  message: Message;
}>();

// ref 指向渲染 markdown 的 DOM 节点，用于查找 Mermaid 占位元素
const contentRef = ref<HTMLElement | null>(null);

/** 渲染当前消息内所有待渲染的 Mermaid 占位块 */
async function renderPendingDiagrams() {
  if (!contentRef.value) return;
  await renderMermaidDiagrams(contentRef.value);
}

// 流式输出结束后（streaming → false）渲染图表
watch(
  () => props.message.streaming,
  async (isStreaming) => {
    if (!isStreaming) {
      await nextTick();
      renderPendingDiagrams();
    }
  }
);

// 组件挂载时渲染历史消息里的图表
onMounted(async () => {
  if (!props.message.streaming) {
    await nextTick();
    renderPendingDiagrams();
  }
  window.addEventListener("message", handleHtmlPreviewMessage);
});

onBeforeUnmount(() => {
  window.removeEventListener("message", handleHtmlPreviewMessage);
});

// 接收 buildHtmlPreviewBlock 注入到 iframe 内的脚本上报的内容高度。
// 只信任 event.source 确实是本组件渲染出的某个 iframe 的 contentWindow，
// 不信任消息内容本身（sandbox 去掉 allow-same-origin 后无法直接读高度）。
function handleHtmlPreviewMessage(event: MessageEvent) {
  const height = (event.data as { __baiyuHtmlPreviewHeight?: unknown } | null)
    ?.__baiyuHtmlPreviewHeight;
  if (typeof height !== "number") return;
  const frames = contentRef.value?.querySelectorAll<HTMLIFrameElement>(".html-preview-frame");
  frames?.forEach((frame) => {
    if (frame.contentWindow === event.source) {
      frame.style.height = `${Math.min(Math.max(height, 120), 600)}px`;
    }
  });
}

// markdown-content 容器上的点击事件委托：处理 buildHtmlPreviewBlock /
// buildMermaidPreviewBlock 生成的"预览/源码"切换按钮。这两个按钮曾经用内联
// onclick 属性实现，但应用的 CSP（script-src 精确哈希白名单，无 unsafe-inline/
// unsafe-hashes）会静默拦截所有内联事件处理器，导致按钮点了没反应；改成这里
// 统一用真实的 Vue @click 监听器（编译进打包后的 JS，不受 CSP 内联限制）。
function handleMarkdownClick(event: MouseEvent) {
  const target = event.target as HTMLElement | null;
  if (!target) return;

  const previewToggle = target.closest<HTMLElement>(".html-preview-toggle");
  if (previewToggle) {
    const block = previewToggle.closest(".html-preview-block");
    if (block) {
      const showingPreview = block.classList.toggle("show-preview");
      previewToggle.textContent = showingPreview ? "查看源码" : "预览效果";
    }
    return;
  }

  const mermaidToggle = target.closest<HTMLElement>(".mermaid-toggle");
  if (mermaidToggle) {
    const block = mermaidToggle.closest(".mermaid-preview-block");
    if (block) {
      const showingDiagram = block.classList.toggle("show-diagram");
      mermaidToggle.textContent = showingDiagram ? "查看源码" : "查看图表";
    }
  }
}

// ============ 计算属性 ============

// 是否为用户消息
const isUser = computed(() => props.message.role === "user");

// 是否为 AI 助手消息
const isAssistant = computed(() => props.message.role === "assistant");

// 渲染后的 Markdown 内容（解析、净化、HTML/Mermaid 预览块生成全部在
// utils/markdown.ts 的 renderMarkdown 里完成）
const renderedContent = computed(() => renderMarkdown(props.message.content));

// ============ 方法函数 ============

// 格式化时间显示
const formatTime = (timestamp: number) => {
  return new Date(timestamp).toLocaleTimeString("zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
  });
};

// 复制状态
const copied = ref(false);

// 复制消息内容
const handleCopy = async () => {
  try {
    await navigator.clipboard.writeText(props.message.content);
    copied.value = true;
    setTimeout(() => {
      copied.value = false;
    }, 2000);
  } catch (err) {
    console.error("Failed to copy:", err);
  }
};
</script>

<template>
  <div
    class="message-wrapper"
    :class="{ 'user-message': isUser }"
  >
    <div class="message-avatar">
      <n-avatar 
        round 
        :size="36" 
        class="avatar"
        :class="{ 'user-avatar': isUser, 'ai-avatar': isAssistant }"
      >
        <n-icon :size="18">
          <Person v-if="isUser" />
          <Sparkles v-else />
        </n-icon>
      </n-avatar>
    </div>

    <div class="message-content">
      <div class="message-header">
        <span class="message-author">{{ isUser ? "你" : "AI 助手" }}</span>
        <span class="message-time">{{ formatTime(message.timestamp) }}</span>
      </div>

      <div
        class="message-body"
        :class="{ 'user-body': isUser }"
      >
        <!-- 思考过程（思考型模型的 reasoning 流式增量；默认折叠，点击展开。
             仅内存态展示，不写入正文、不入库，刷新后消失） -->
        <details
          v-if="message.thinking"
          class="thinking-block"
        >
          <summary class="thinking-summary">
            思考过程
          </summary>
          <pre class="thinking-content">{{ message.thinking }}</pre>
        </details>

        <div
          ref="contentRef"
          class="markdown-content"
          @click="handleMarkdownClick"
          v-html="renderedContent"
        />
        
        <!-- Streaming indicator -->
        <div
          v-if="message.streaming"
          class="streaming-indicator"
        >
          <n-spin size="small" />
          <span class="streaming-text">思考中...</span>
        </div>

        <!-- 工具调用列表 -->
        <div
          v-if="message.toolCalls && message.toolCalls.length > 0"
          class="tool-calls"
        >
          <div
            v-for="tc in message.toolCalls"
            :key="tc.callId"
            class="tool-call-item"
            :class="{ 'tool-call-error': tc.status === 'error' }"
          >
            <div class="tool-call-header">
              <n-spin
                v-if="tc.status === 'calling'"
                :size="12"
              />
              <span class="tool-call-status-icon">{{ tc.status === "done" ? "✓" : tc.status === "error" ? "✕" : "" }}</span>
              <span class="tool-call-name">{{ tc.toolName }}</span>
              <span class="tool-call-status-text">{{
                tc.status === "calling" ? "调用中" : tc.status === "error" ? "调用失败" : "已完成"
              }}</span>
            </div>
            <pre class="tool-call-args">{{ tc.arguments }}</pre>
            <pre
              v-if="tc.result"
              class="tool-call-result"
            >{{ tc.result }}</pre>
          </div>
        </div>
      </div>

      <!-- Error message -->
      <div
        v-if="message.error"
        class="message-error"
      >
        <n-alert
          type="error"
          :show-icon="true"
          :bordered="false"
        >
          {{ message.error }}
        </n-alert>
      </div>

      <!-- Actions -->
      <div
        v-if="!isUser && !message.streaming"
        class="message-actions"
      >
        <n-tooltip
          placement="top"
          :show="copied"
        >
          <template #trigger>
            <button
              class="action-btn"
              title="复制"
              @click="handleCopy"
            >
              <n-icon :size="14">
                <Copy />
              </n-icon>
            </button>
          </template>
          <span>已复制!</span>
        </n-tooltip>
      </div>
    </div>
  </div>
</template>

<style scoped lang="scss">
.message-wrapper {
  display: flex;
  gap: 16px;
  padding: 20px 0;
  animation: message-enter $duration-slow $ease both;
}

// 入场: opacity 0→1 + translateY(40px)→0 + scale(0.95)→1
@keyframes message-enter {
  from {
    opacity: 0;
    transform: translateY(40px) scale(0.95);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}

.message-wrapper.user-message {
  flex-direction: row-reverse;

  .message-content {
    align-items: flex-end;
  }

  .message-header {
    flex-direction: row-reverse;
  }
}

.avatar {
  flex-shrink: 0;
}

.user-avatar {
  background: $ink;
  color: $bg;
}

.ai-avatar {
  background: $bg;
  color: $ink;
  border: $border;
}

.message-content {
  flex: 1;
  max-width: calc(100% - 80px);
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.message-header {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  padding: 0 4px;
}

.message-author {
  font-family: $font-serif;
  font-weight: 700;
  color: $ink;
}

.message-time {
  color: $ink-faint;
  font-size: 12px;
  font-family: $font-mono;
}

.message-body {
  padding: 16px 20px;
  background: $bg;
  border: $border-soft;
  word-break: break-word;
  line-height: $leading-body;
  transition:
    transform $duration $ease,
    box-shadow $duration $ease;
}

.message-body:hover {
  transform: translateY(-4px);
  box-shadow: $shadow-hover;
}

.message-body.user-body {
  background: $surface;
  border: $border;
}

.markdown-content {
  color: $ink;
}

.markdown-content :deep(p) {
  margin: 0 0 12px 0;
}

.markdown-content :deep(p:last-child) {
  margin-bottom: 0;
}

/* ===== HTML 预览块 ===== */
.markdown-content :deep(.html-preview-block) {
  border: $border-soft;
  overflow: hidden;
  margin: 12px 0;

  .html-preview-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 12px;
    background: $surface;
    border-bottom: $border-faint;
    user-select: none;
  }

  .html-preview-toggle {
    font-size: 12px;
    padding: 3px 10px;
    border: 1px solid rgba(0, 0, 0, 0.6);
    background: transparent;
    color: $ink-soft;
    cursor: pointer;
    transition: background $duration $ease, color $duration $ease;

    &:hover {
      background: $ink;
      color: $bg;
    }
  }

  .html-lang-badge {
    font-size: 11px;
    color: $ink-faint;
    font-family: $font-mono;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  /* 源码视图：pre 撑满，iframe 隐藏 */
  .html-source-pre {
    margin: 0 !important;
    border-radius: 0 !important;
    border: none !important;
    border-top: none !important;
  }

  .html-preview-frame {
    display: none;
    width: 100%;
    min-height: 120px;
    border: none;
    background: #fff;
  }

  /* 切换到预览模式 */
  &.show-preview {
    .html-source-pre { display: none; }
    .html-preview-frame { display: block; }
  }
}

/* ===== Mermaid 预览块 ===== */
.markdown-content :deep(.mermaid-preview-block) {
  border: $border-soft;
  overflow: hidden;
  margin: 12px 0;

  .mermaid-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 12px;
    background: $surface;
    border-bottom: $border-faint;
    user-select: none;
  }

  .mermaid-toggle {
    font-size: 12px;
    padding: 3px 10px;
    border: 1px solid rgba(0, 0, 0, 0.6);
    background: transparent;
    color: $ink-soft;
    cursor: pointer;
    transition: background $duration $ease, color $duration $ease;

    &:hover {
      background: $ink;
      color: $bg;
    }
  }

  .mermaid-lang-badge {
    font-size: 11px;
    color: $ink-faint;
    font-family: $font-mono;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  /* 源码 pre 默认隐藏（diagram 模式） */
  .mermaid-source-pre {
    display: none;
    margin: 0 !important;
    border-radius: 0 !important;
    border: none !important;
  }

  /* 图表容器：居中显示 SVG，自适应高度 */
  .mermaid-diagram {
    display: flex;
    justify-content: center;
    align-items: center;
    min-height: 80px;
    padding: 20px;
    background: $bg;

    svg {
      max-width: 100%;
      height: auto;
    }
  }

  .mermaid-error {
    color: $ink;
    font-size: 12px;
    padding: 0;
    background: transparent;
    border: none;
    white-space: pre-wrap;
    word-break: break-all;
  }

  /* 切换到源码模式 */
  &:not(.show-diagram) {
    .mermaid-source-pre { display: block; }
    .mermaid-diagram { display: none; }
  }
}

.markdown-content :deep(pre) {
  background: $surface;
  padding: 16px;
  margin: 12px 0;
  overflow-x: auto;
  border: $border-faint;
}

.markdown-content :deep(code) {
  font-family: $font-mono;
  font-size: 0.9em;
}

.markdown-content :deep(pre code) {
  background: transparent;
  padding: 0;
  color: $ink;
}

.markdown-content :deep(:not(pre) > code) {
  background: $surface;
  padding: 3px 6px;
  border: 1px solid rgba(0, 0, 0, 0.2);
  color: $ink;
}

.markdown-content :deep(ul),
.markdown-content :deep(ol) {
  margin: 12px 0;
  padding-left: 24px;
}

.markdown-content :deep(li) {
  margin: 6px 0;
}

.markdown-content :deep(blockquote) {
  margin: 12px 0;
  padding: 12px 16px;
  border-left: 2px solid $ink;
  background: $surface;
  color: $ink-soft;
}

.markdown-content :deep(table) {
  width: 100%;
  border-collapse: collapse;
  margin: 12px 0;
  overflow: hidden;
}

.markdown-content :deep(th),
.markdown-content :deep(td) {
  border: 1px solid rgba(0, 0, 0, 0.6);
  padding: 10px 14px;
  text-align: left;
}

.markdown-content :deep(th) {
  background: $surface;
  font-weight: 600;
  font-size: 0.8rem;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.markdown-content :deep(tr:nth-child(even)) {
  background: $surface;
}

/* 思考过程折叠区 - 黑白灰 + 排版层次，与工具调用块同一视觉家族 */
.thinking-block {
  margin-bottom: 12px;
  border: $border-faint;
  background: $surface;
}

.thinking-summary {
  padding: 6px 12px;
  font-family: $font-mono;
  font-size: 11px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  color: $ink-faint;
  cursor: pointer;
  user-select: none;
  transition: color $duration $ease;

  &:hover {
    color: $ink;
  }
}

.thinking-content {
  margin: 0;
  padding: 10px 12px;
  border-top: $border-faint;
  font-size: 12px;
  line-height: $leading-body;
  color: $ink-soft;
  white-space: pre-wrap;
  word-break: break-word;
}

.streaming-indicator {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px dashed rgba(0, 0, 0, 0.4);
  color: $ink-faint;
  font-size: 13px;
}

.tool-calls {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px dashed rgba(0, 0, 0, 0.4);
}

.tool-call-item {
  border: $border-faint;
  background: $surface;
  padding: 10px 12px;
}

.tool-call-item.tool-call-error {
  border-color: $ink;
}

.tool-call-header {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}

.tool-call-status-icon {
  color: $ink-soft;
  font-family: $font-mono;
}

.tool-call-name {
  font-family: $font-mono;
  font-weight: 600;
  color: $ink;
}

.tool-call-status-text {
  color: $ink-faint;
  letter-spacing: 0.05em;
}

.tool-call-args,
.tool-call-result {
  margin: 6px 0 0 0;
  padding: 8px 10px;
  background: $bg;
  border: 1px solid rgba(0, 0, 0, 0.15);
  font-family: $font-mono;
  font-size: 12px;
  color: $ink-soft;
  white-space: pre-wrap;
  word-break: break-all;
}

.message-error {
  margin-top: 8px;
}

.message-actions {
  display: flex;
  gap: 8px;
  padding: 4px;
  opacity: 0;
  transition: opacity 0.2s;
}

.message-wrapper:hover .message-actions {
  opacity: 1;
}

.action-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: 1px solid rgba(0, 0, 0, 0.4);
  background: $bg;
  color: $ink-soft;
  cursor: pointer;
  transition:
    background $duration $ease,
    color $duration $ease;
}

.streaming-text {
  color: $ink-faint;
  font-size: 13px;
}

.action-btn:hover {
  background: $ink;
  color: $bg;
}
</style>
