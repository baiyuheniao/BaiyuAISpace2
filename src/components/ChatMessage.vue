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

// 导入 Markdown 解析库
import { marked } from "marked";

// 导入 LaTeX 公式渲染扩展
import markedKatex from "marked-katex-extension";
import "katex/dist/katex.min.css";

// 导入 HTML 净化库
import DOMPurify from "dompurify";

// 导入代码高亮库
import hljs from "highlight.js";
// 灰度高亮主题，契合黑白设计系统
import "highlight.js/styles/grayscale.css";

// 导入 Mermaid 图表库
import mermaid from "mermaid";

// 导入消息类型
import type { Message } from "@/stores/chat";

// 导入图标
import { Person, Sparkles, Copy } from "@vicons/ionicons5";

// ============ Props 定义 ============

const props = defineProps<{
  message: Message;
}>();

// ============ Mermaid 初始化 ============

mermaid.initialize({
  startOnLoad: false,
  // neutral 主题为灰度配色，契合黑白设计系统
  theme: "neutral",
  securityLevel: "loose",
  fontFamily: '"Inter Variable", "Inter", system-ui, sans-serif',
});

// DOMPurify 默认的 URI 安全校验会把 srcdoc 这种"值不是 URL 而是一整段
// HTML"的属性直接剥离，用 hook 强制放行 -- buildHtmlPreviewBlock 生成的
// srcdoc 内容里的引号已经手动转义过，不依赖这里的校验来防属性逃逸。
DOMPurify.addHook("uponSanitizeAttribute", (_node, data) => {
  if (data.attrName === "srcdoc") {
    data.forceKeepAttr = true;
  }
});

// ref 指向渲染 markdown 的 DOM 节点，用于查找 Mermaid 占位元素
const contentRef = ref<HTMLElement | null>(null);

// 唯一 ID 计数器，mermaid.render() 要求每次传不同 id
let mermaidIdCounter = 0;

/** 查找并渲染当前消息内所有待渲染的 Mermaid 占位块 */
async function renderMermaidDiagrams() {
  if (!contentRef.value) return;
  const pending = contentRef.value.querySelectorAll<HTMLElement>(
    ".mermaid-diagram[data-pending='true']"
  );
  for (const el of pending) {
    const code = decodeURIComponent(el.getAttribute("data-code") ?? "");
    if (!code) continue;
    el.setAttribute("data-pending", "false");
    const id = `mermaid-${++mermaidIdCounter}`;
    try {
      const { svg } = await mermaid.render(id, code);
      el.innerHTML = svg;
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      el.innerHTML = `<pre class="mermaid-error">图表渲染失败:\n${msg}</pre>`;
    }
  }
}

// 流式输出结束后（streaming → false）渲染图表
watch(
  () => props.message.streaming,
  async (isStreaming) => {
    if (!isStreaming) {
      await nextTick();
      renderMermaidDiagrams();
    }
  }
);

// 组件挂载时渲染历史消息里的图表
onMounted(async () => {
  if (!props.message.streaming) {
    await nextTick();
    renderMermaidDiagrams();
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

// ============ HTML 预览块生成 ============

/**
 * 为 HTML 代码块生成"源码 + iframe 预览"双视图结构。
 * iframe 使用 sandbox + srcdoc 隔离执行，toolbar 按钮的点击由
 * handleMarkdownClick 事件委托处理（不用内联 onclick，会被 CSP 拦截）。
 */
function buildHtmlPreviewBlock(code: string): string {
  const highlighted = hljs.highlight(code, { language: "html" }).value;

  // 如果模型只输出了 HTML 片段（非完整文档），补全基础框架以确保正确渲染
  const isFullDoc = /^\s*<!doctype/i.test(code) || /^\s*<html[\s>]/i.test(code);
  const docHtml = isFullDoc
    ? code
    : `<!DOCTYPE html><html><head><meta charset="utf-8"><style>body{margin:16px;font-family:system-ui,sans-serif;line-height:1.6}</style></head><body>${code}</body></html>`;

  // sandbox 不含 allow-same-origin（Issue #55：曾允许 iframe 内脚本靠
  // allow-same-origin 逃出沙箱读父页面 localStorage）。iframe 因此是不透明源，
  // 父页面无法用 contentDocument 读高度，改为在 iframe 内加载一个外部脚本文件，
  // 通过 postMessage 主动上报高度（父页面处理见 handleHtmlPreviewMessage）。
  // 必须是外部文件而不是内联 <script>：应用的 CSP 是精确哈希白名单（不含
  // unsafe-inline/unsafe-hashes），且 srcdoc 文档会继承父页面 CSP，内联脚本
  // 一律会被拦截静默失效；同源静态文件走 src= 加载则天然匹配 script-src 'self'。
  // 闭合标签拆成两段字符串，避免在 .vue 的 <script> 块里出现完整闭合标签字面量。
  const reporterTag = '<script src="/html-preview-reporter.js"></' + "script>";
  const docWithReporter = /<\/body>/i.test(docHtml)
    ? docHtml.replace(/<\/body>/i, `${reporterTag}</body>`)
    : docHtml + reporterTag;

  // srcdoc 属性值内只需转义 & 和 "
  const srcdoc = docWithReporter.replace(/&/g, "&amp;").replace(/"/g, "&quot;");

  // 按钮不再用内联 onclick（同样会被 CSP 拦截），改为在 handleMarkdownClick 里
  // 用事件委托处理 .html-preview-toggle 点击。
  return (
    `<div class="html-preview-block">` +
    `<div class="html-preview-toolbar">` +
    `<button class="html-preview-toggle">预览效果</button>` +
    `<span class="html-lang-badge">HTML</span>` +
    `</div>` +
    `<pre class="html-source-pre"><code class="hljs language-html">${highlighted}</code></pre>` +
    `<iframe class="html-preview-frame" srcdoc="${srcdoc}" ` +
    `sandbox="allow-scripts allow-modals allow-forms"></iframe>` +
    `</div>`
  );
}

// ============ Mermaid 预览块生成 ============

/**
 * 为 Mermaid 代码块生成"图表预览 + 源码"双视图结构。
 * 默认展示渲染后的图表（show-diagram 类），点击按钮切换。
 * 图表占位元素 data-pending="true"，由 renderMermaidDiagrams() 在 DOM 就绪后填充。
 */
function buildMermaidPreviewBlock(code: string): string {
  const highlighted = hljs.highlight(code, { language: "plaintext" }).value;
  // encodeURIComponent 保证特殊字符在 data-* 属性里安全传递
  const encoded = encodeURIComponent(code);

  // 按钮不再用内联 onclick（CSP 会拦截），改为在 handleMarkdownClick 里事件委托处理。
  return (
    `<div class="mermaid-preview-block show-diagram">` +
    `<div class="mermaid-toolbar">` +
    `<button class="mermaid-toggle">查看源码</button>` +
    `<span class="mermaid-lang-badge">Mermaid</span>` +
    `</div>` +
    `<pre class="mermaid-source-pre"><code class="hljs">${highlighted}</code></pre>` +
    `<div class="mermaid-diagram" data-pending="true" data-code="${encoded}"></div>` +
    `</div>`
  );
}

// ============ Markdown 配置 ============

// marked 默认把消息正文里出现的裸 HTML（不在围栏代码块内的）原样透传到输出，
// 这里转义掉，防止 <script>/<img onerror>/<style> 等直接变成真实 DOM 节点。
// 围栏代码块走的是下面的 code() 渲染器，不受影响。
function escapeRawHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

// 不少模型（如 DeepSeek）输出 LaTeX 公式时用的是 \[...\] / \(...\) 定界符而非
// $$...$$ / $...$。marked-katex-extension 只认后者；更麻烦的是 CommonMark 规则
// 会把 "\[" "\]" 这类反斜杠+标点解析成转义序列（渲染为去掉反斜杠的纯标点），
// 定界符在 marked 的分词阶段之前就已经丢失了。所以要在喂给 marked 之前，
// 先把 \[...\] / \(...\) 正规化成 $$...$$ / $...$。
function normalizeLatexDelimiters(text: string): string {
  return text
    .replace(/\\\[([\s\S]*?)\\\]/g, (_match, expr: string) => `$$${expr}$$`)
    .replace(/\\\(([\s\S]*?)\\\)/g, (_match, expr: string) => `$${expr}$`);
}

// 使用单例确保 marked.use() 只调用一次（marked 修改全局实例）
const markedInitPromise = (() => {
  let promise: Promise<void> | null = null;
  return () => {
    if (!promise) {
      promise = new Promise((resolve) => {
        // 黑白设计系统下不用 KaTeX 默认的红色报错色，退化为普通灰字
        marked.use(markedKatex({ throwOnError: false, errorColor: "#444444" }));
        marked.use({
          renderer: {
            // 消息正文中的裸 HTML（块级或内联）一律转义为纯文本
            html(html: string, _block?: boolean): string {
              return escapeRawHtml(html);
            },
            // 处理所有围栏代码块（三参数形式是此版本 marked 的 Renderer 签名）
            code(text: string, lang: string | undefined, _escaped: boolean): string {
              // 取 info string 第一个单词作为语言标识（如 "html filename.html" → "html"）
              const normalizedLang = ((lang ?? "").trim().split(/\s+/)[0] ?? "").toLowerCase();

              if (normalizedLang === "html" || normalizedLang === "htm") {
                return buildHtmlPreviewBlock(text);
              }

              if (normalizedLang === "mermaid") {
                return buildMermaidPreviewBlock(text);
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
// 二次防线：即便 marked 的转义有疏漏，DOMPurify 仍会按白名单剥离危险标签/属性。
// iframe/sandbox/srcdoc 是 buildHtmlPreviewBlock 生成的沙箱预览所必需的，显式放行。
const renderedContent = computed(() => {
  if (!props.message.content) return "";
  const normalized = normalizeLatexDelimiters(props.message.content);
  const html = marked.parse(normalized, { async: false, breaks: true }) as string;
  return DOMPurify.sanitize(html, {
    ADD_TAGS: ["iframe"],
    ADD_ATTR: ["sandbox", "srcdoc"],
    // srcdoc's value is a full HTML document (it legitimately contains
    // "</style>" etc.), which DOMPurify's SAFE_FOR_XML close-tag probe
    // otherwise treats as an attribute-escape attempt and strips outright.
    // The uponSanitizeAttribute hook above is what actually keeps srcdoc.
    SAFE_FOR_XML: false,
  });
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
          ref="contentRef"
          class="markdown-content"
          v-html="renderedContent"
          @click="handleMarkdownClick"
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
