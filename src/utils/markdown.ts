// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// 聊天消息的 Markdown 渲染管线（marked 配置、HTML/Mermaid 预览块、DOMPurify
// 净化、Mermaid 图表渲染）。
//
// 必须放在独立模块而不是 ChatMessage.vue 的 <script setup> 里：setup 块的
// 顶层语句会在**每个组件实例创建时**执行，而 marked.use() / DOMPurify.addHook()
// / mermaid.initialize() 改的都是全局单例——放在组件里意味着每挂载一条消息
// 就给 marked 多包一层扩展、给 DOMPurify 多挂一个重复钩子，长会话下 Markdown
// 解析会一条比一条慢。模块顶层代码在整个应用生命周期里只执行一次。

import { marked } from "marked";
import markedKatex from "marked-katex-extension";
import "katex/dist/katex.min.css";
import DOMPurify from "dompurify";
import hljs from "highlight.js";
// 灰度高亮主题，契合黑白设计系统
import "highlight.js/styles/grayscale.css";
import mermaid from "mermaid";

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

/**
 * 为 HTML 代码块生成"源码 + iframe 预览"双视图结构。
 * iframe 使用 sandbox + srcdoc 隔离执行，toolbar 按钮的点击由
 * ChatMessage.vue 的事件委托处理（不用内联 onclick，会被 CSP 拦截）。
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
  // 通过 postMessage 主动上报高度（父页面处理见 ChatMessage.vue 的
  // handleHtmlPreviewMessage）。
  // 必须是外部文件而不是内联 <script>：应用的 CSP 是精确哈希白名单（不含
  // unsafe-inline/unsafe-hashes），且 srcdoc 文档会继承父页面 CSP，内联脚本
  // 一律会被拦截静默失效；同源静态文件走 src= 加载则天然匹配 script-src 'self'。
  // 闭合标签拆成两段字符串，避免源文件里出现完整闭合标签字面量被工具误判。
  const reporterTag = '<script src="/html-preview-reporter.js"></' + "script>";
  const docWithReporter = /<\/body>/i.test(docHtml)
    ? docHtml.replace(/<\/body>/i, `${reporterTag}</body>`)
    : docHtml + reporterTag;

  // srcdoc 属性值内只需转义 & 和 "
  const srcdoc = docWithReporter.replace(/&/g, "&amp;").replace(/"/g, "&quot;");

  // 按钮不用内联 onclick（会被 CSP 拦截），由 ChatMessage.vue 里
  // handleMarkdownClick 用事件委托处理 .html-preview-toggle 点击。
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

/**
 * 为 Mermaid 代码块生成"图表预览 + 源码"双视图结构。
 * 默认展示渲染后的图表（show-diagram 类），点击按钮切换。
 * 图表占位元素 data-pending="true"，由 renderMermaidDiagrams() 在 DOM 就绪后填充。
 */
function buildMermaidPreviewBlock(code: string): string {
  const highlighted = hljs.highlight(code, { language: "plaintext" }).value;
  // encodeURIComponent 保证特殊字符在 data-* 属性里安全传递
  const encoded = encodeURIComponent(code);

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

// marked.use() 修改的是全局 marked 实例，模块顶层保证整个应用只配置一次。
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

/**
 * 把消息正文渲染成净化后的 HTML。
 * 二次防线：即便 marked 的转义有疏漏，DOMPurify 仍会按白名单剥离危险标签/属性。
 * iframe/sandbox/srcdoc 是 buildHtmlPreviewBlock 生成的沙箱预览所必需的，显式放行。
 */
export function renderMarkdown(content: string): string {
  if (!content) return "";
  const normalized = normalizeLatexDelimiters(content);
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
}

// 唯一 ID 计数器，mermaid.render() 要求每次传不同 id；模块级保证全局唯一
let mermaidIdCounter = 0;

/** 查找并渲染容器内所有待渲染的 Mermaid 占位块 */
export async function renderMermaidDiagrams(container: HTMLElement): Promise<void> {
  const pending = container.querySelectorAll<HTMLElement>(
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
