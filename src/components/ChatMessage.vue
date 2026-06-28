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
import { computed, ref } from "vue";

// 导入 NaiveUI 组件
import { NAvatar, NIcon, NSpin, NAlert, NTooltip } from "naive-ui";

// 导入 Markdown 解析库
import { marked } from "marked";

// 导入代码高亮库
import hljs from "highlight.js";
import "highlight.js/styles/github-dark.css";

// 导入消息类型
import type { Message } from "@/stores/chat";

// 导入图标
import { Person, Sparkles, Copy } from "@vicons/ionicons5";

// ============ Props 定义 ============

const props = defineProps<{
  message: Message;
}>();

// ============ HTML 预览块生成 ============

/**
 * 为 HTML 代码块生成"源码 + iframe 预览"双视图结构。
 * iframe 使用 sandbox + srcdoc 隔离执行，toolbar 按钮通过内联
 * onclick 切换 show-preview 类来控制显示哪一侧。
 */
function buildHtmlPreviewBlock(code: string): string {
  const highlighted = hljs.highlight(code, { language: "html" }).value;

  // 如果模型只输出了 HTML 片段（非完整文档），补全基础框架以确保正确渲染
  const isFullDoc = /^\s*<!doctype/i.test(code) || /^\s*<html[\s>]/i.test(code);
  const docHtml = isFullDoc
    ? code
    : `<!DOCTYPE html><html><head><meta charset="utf-8"><style>body{margin:16px;font-family:system-ui,sans-serif;line-height:1.6}</style></head><body>${code}</body></html>`;

  // srcdoc 属性值内只需转义 & 和 "
  const srcdoc = docHtml.replace(/&/g, "&amp;").replace(/"/g, "&quot;");

  // onload 尝试按内容高度自适应（不超过 600px），sandbox allow-same-origin
  // 让父页面能读 contentDocument.scrollHeight
  const onload =
    "try{var h=this.contentDocument.documentElement.scrollHeight" +
    "||this.contentDocument.body.scrollHeight;" +
    "this.style.height=Math.min(Math.max(h,120),600)+'px'}catch(e){}";

  const onclick =
    "var b=this.closest('.html-preview-block');" +
    "b.classList.toggle('show-preview');" +
    "this.textContent=b.classList.contains('show-preview')?'查看源码':'预览效果'";

  return (
    `<div class="html-preview-block">` +
    `<div class="html-preview-toolbar">` +
    `<button class="html-preview-toggle" onclick="${onclick}">预览效果</button>` +
    `<span class="html-lang-badge">HTML</span>` +
    `</div>` +
    `<pre class="html-source-pre"><code class="hljs language-html">${highlighted}</code></pre>` +
    `<iframe class="html-preview-frame" srcdoc="${srcdoc}" ` +
    `sandbox="allow-scripts allow-same-origin allow-modals allow-forms" ` +
    `onload="${onload}"></iframe>` +
    `</div>`
  );
}

// ============ Markdown 配置 ============

// 使用单例确保 marked.use() 只调用一次（marked 修改全局实例）
const markedInitPromise = (() => {
  let promise: Promise<void> | null = null;
  return () => {
    if (!promise) {
      promise = new Promise((resolve) => {
        marked.use({
          renderer: {
            // 处理所有围栏代码块（三参数形式是此版本 marked 的 Renderer 签名）
            code(text: string, lang: string | undefined, _escaped: boolean): string {
              // 取 info string 第一个单词作为语言标识（如 "html filename.html" → "html"）
              const normalizedLang = ((lang ?? "").trim().split(/\s+/)[0] ?? "").toLowerCase();

              if (normalizedLang === "html" || normalizedLang === "htm") {
                return buildHtmlPreviewBlock(text);
              }

              // 其余语言走 highlight.js 常规高亮
              const language = hljs.getLanguage(normalizedLang) ? normalizedLang : "plaintext";
              const highlighted = hljs.highlight(text, { language }).value;
              return `<pre><code class="hljs language-${language}">${highlighted}</code></pre>`;
            },
          },
        });
        resolve();
      });
    }
    return promise;
  };
})();

// 初始化 marked 配置
markedInitPromise();

// ============ 计算属性 ============

// 是否为用户消息
const isUser = computed(() => props.message.role === "user");

// 是否为 AI 助手消息
const isAssistant = computed(() => props.message.role === "assistant");

// 渲染后的 Markdown 内容
const renderedContent = computed(() => {
  if (!props.message.content) return "";
  return marked.parse(props.message.content, { async: false, breaks: true }) as string;
});

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
        <div
          class="markdown-content"
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
  animation: fadeIn 0.3s ease;
}

@keyframes fadeIn {
  from {
    opacity: 0;
    transform: translateY(10px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
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
  background: #000000;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
}

.ai-avatar {
  background: #333333;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
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
  font-weight: 600;
  color: var(--n-text-color-1);
}

.message-time {
  color: var(--n-text-color-3);
  font-size: 12px;
}

.message-body {
  padding: 16px 20px;
  background: var(--n-color-embed);
  border-radius: $radius-xl;
  border-bottom-left-radius: $radius-sm;
  word-break: break-word;
  line-height: 1.7;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.04);
  transition: box-shadow 0.2s;
}

.message-body:hover {
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.08);
}

.message-body.user-body {
  background: rgba(0, 0, 0, 0.05);
  border-bottom-left-radius: $radius-xl;
  border-bottom-right-radius: $radius-sm;
}

.markdown-content {
  color: var(--n-text-color-1);
}

.markdown-content :deep(p) {
  margin: 0 0 12px 0;
}

.markdown-content :deep(p:last-child) {
  margin-bottom: 0;
}

/* ===== HTML 预览块 ===== */
.markdown-content :deep(.html-preview-block) {
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: $radius-lg;
  overflow: hidden;
  margin: 12px 0;

  .html-preview-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 12px;
    background: #252535;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    user-select: none;
  }

  .html-preview-toggle {
    font-size: 12px;
    padding: 3px 10px;
    border-radius: $radius-sm;
    border: 1px solid rgba(255, 255, 255, 0.2);
    background: transparent;
    color: #a6adc8;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;

    &:hover {
      background: rgba(255, 255, 255, 0.1);
      color: #cdd6f4;
    }
  }

  .html-lang-badge {
    font-size: 11px;
    color: rgba(255, 255, 255, 0.3);
    font-family: "JetBrains Mono", "Fira Code", "Consolas", monospace;
    letter-spacing: 0.04em;
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

.markdown-content :deep(pre) {
  background: #1e1e2e;
  border-radius: $radius-lg;
  padding: 16px;
  margin: 12px 0;
  overflow-x: auto;
  border: 1px solid rgba(255, 255, 255, 0.1);
}

.markdown-content :deep(code) {
  font-family: "JetBrains Mono", "Fira Code", "Consolas", monospace;
  font-size: 0.9em;
}

.markdown-content :deep(pre code) {
  background: transparent;
  padding: 0;
  color: #cdd6f4;
}

.markdown-content :deep(:not(pre) > code) {
  background: rgba(128, 128, 128, 0.15);
  padding: 3px 6px;
  border-radius: $radius-sm;
  color: var(--n-text-color-1);
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
  border-left: 4px solid #000000;
  background: rgba(0, 0, 0, 0.04);
  border-radius: 0 $radius-md $radius-md 0;
}

.markdown-content :deep(table) {
  width: 100%;
  border-collapse: collapse;
  margin: 12px 0;
  border-radius: $radius-md;
  overflow: hidden;
}

.markdown-content :deep(th),
.markdown-content :deep(td) {
  border: 1px solid var(--n-border-color);
  padding: 10px 14px;
  text-align: left;
}

.markdown-content :deep(th) {
  background: rgba(24, 160, 88, 0.1);
  font-weight: 600;
}

.markdown-content :deep(tr:nth-child(even)) {
  background: var(--n-color-embed);
}

.streaming-indicator {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px dashed var(--n-border-color);
  color: var(--n-text-color-3);
  font-size: 13px;
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
  border: none;
  border-radius: $radius-sm;
  background: transparent;
  color: var(--n-text-color-3);
  cursor: pointer;
  transition: all 0.2s;
}

.streaming-text {
  color: var(--n-text-color-3);
  font-size: 13px;
}

.action-btn:hover {
  background: var(--n-color-embed);
  color: var(--n-text-color-1);
}
</style>
