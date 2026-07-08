// 由 ChatMessage.vue 的 buildHtmlPreviewBlock() 通过 <script src="/html-preview-reporter.js">
// 注入到 HTML 预览 iframe（srcdoc，sandbox 不含 allow-same-origin）里。
// 作为同源静态文件加载能匹配 CSP 的 script-src 'self'，
// 而不像内联 <script> 那样需要构建期哈希、也不能靠内联脚本执行。
(function () {
  function report() {
    try {
      var h = document.documentElement.scrollHeight || document.body.scrollHeight;
      parent.postMessage({ __baiyuHtmlPreviewHeight: h }, "*");
    } catch (e) {
      // 不透明源 iframe 里 postMessage 本身不受同源限制，这里只是兜底防御未知环境异常
    }
  }
  window.addEventListener("load", report);
  window.addEventListener("resize", report);
  report();
})();
