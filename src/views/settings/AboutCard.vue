<!-- This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. -->

<!--
  AboutCard.vue - 关于卡片：版本信息、检测最新版本（含弹窗）、导出日志
  （从 SettingsView.vue 拆出，逻辑与模板原样搬迁。）
-->

<script setup lang="ts">
import { ref, onBeforeUnmount, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { save } from "@tauri-apps/plugin-dialog";
import { open as openExternalUrl } from "@tauri-apps/plugin-shell";
import { check as checkTauriUpdate, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { NCard, NButton, NSpace, NTag, NModal, NIcon, NText, NEmpty } from "naive-ui";
import { useMessage } from "@/composables/useNotify";
import {
  InformationCircleOutline,
} from "@vicons/ionicons5";

const message = useMessage();

// ============ 日志导出 ============

const exportLogs = async () => {
  try {
    // 让用户选择保存位置
    const filePath = await save({
      defaultPath: `BaiyuAISpace2_logs_${new Date().toISOString().split('T')[0]}.log`,
      filters: [{ name: "Log Files", extensions: ["log"] }]
    });
    
    if (filePath) {
      // 调用后端复制日志文件
      const result = await invoke<string>("copy_log_file", { destPath: filePath });
      message.success("日志已导出到: " + result);
    }
  } catch (error) {
    message.error("导出日志失败: " + error);
  }
};

// ============ 检测最新版本 ============

interface ReleaseInfo {
  version: string;
  name: string;
  body: string;
  htmlUrl: string;
  publishedAt: string | null;
}

interface LatestReleasesResult {
  currentVersion: string;
  stable: ReleaseInfo | null;
  beta: ReleaseInfo | null;
}

/** 当前运行的应用版本号，启动时读取一次用于展示和比对 */
const currentAppVersion = ref("");

onMounted(async () => {
  try {
    currentAppVersion.value = await getVersion();
  } catch {
    // 读取失败不影响其他功能，留空即可
  }
});

const checkingUpdate = ref(false);
const showUpdateModal = ref(false);
const latestReleases = ref<LatestReleasesResult | null>(null);

/** 更新内容以对应 GitHub Release 页面的原文为准，用 marked 渲染成 HTML 再净化 */
const renderReleaseNotes = (body: string): string => {
  const html = marked.parse(body.trim() || "暂无更新说明。", { async: false, breaks: true }) as string;
  return DOMPurify.sanitize(html);
};

/**
 * 简化版 semver 比较：核心版本号（major.minor.patch）逐段比较；核心相同时，
 * 不带 -beta.N 后缀的正式版视为比任何 beta 都新，两边都有后缀则逐段比较
 * （数字段按数值比，否则按字典序）。返回值同 Array.sort 的比较器语义。
 * 用于判断 GitHub 上查到的版本是否比本地正在运行的版本更新——不能简单用
 * 字符串不等于判断，否则本地跑着尚未发布的开发版（比如 beta.5）时，
 * 会把已发布的旧版本（beta.4）误判成"有新版本"。
 */
const compareVersions = (a: string, b: string): number => {
  const parse = (v: string) => {
    const dashIndex = v.indexOf("-");
    const core = dashIndex === -1 ? v : v.slice(0, dashIndex);
    const pre = dashIndex === -1 ? null : v.slice(dashIndex + 1).split(".");
    return { core: core.split(".").map((n) => Number.parseInt(n, 10) || 0), pre };
  };
  const va = parse(a);
  const vb = parse(b);

  for (let i = 0; i < Math.max(va.core.length, vb.core.length); i++) {
    const diff = (va.core[i] ?? 0) - (vb.core[i] ?? 0);
    if (diff !== 0) return diff;
  }

  if (va.pre === null && vb.pre === null) return 0;
  if (va.pre === null) return 1;
  if (vb.pre === null) return -1;

  for (let i = 0; i < Math.max(va.pre.length, vb.pre.length); i++) {
    const sa = va.pre[i];
    const sb = vb.pre[i];
    if (sa === undefined) return -1;
    if (sb === undefined) return 1;
    const na = Number(sa);
    const nb = Number(sb);
    if (!Number.isNaN(na) && !Number.isNaN(nb)) {
      if (na !== nb) return na - nb;
    } else if (sa !== sb) {
      return sa < sb ? -1 : 1;
    }
  }
  return 0;
};

const isNewerVersion = (remoteVersion: string): boolean =>
  compareVersions(remoteVersion, currentAppVersion.value) > 0;

const checkLatestVersion = async () => {
  checkingUpdate.value = true;
  try {
    latestReleases.value = await invoke<LatestReleasesResult>("check_latest_releases");
    if (!latestReleases.value.stable && !latestReleases.value.beta) {
      message.warning("未能获取到任何已发布的版本信息");
      return;
    }
    showUpdateModal.value = true;
  } catch (error) {
    message.error("检测最新版本失败: " + error);
  } finally {
    checkingUpdate.value = false;
  }
};

const openReleasePage = (url: string) => {
  void openExternalUrl(url);
};

/** 是否正在下载/安装更新（安装期间禁止重复点击） */
const installingUpdate = ref(false);

/** 安装进度提示文案 */
const installProgress = ref("");

/**
 * 检测到新版本后点击"立即更新并安装"：更新包走 Tauri updater 插件默认的
 * endpoint（tauri.conf.json 里配置的 updater-manifest，只跟踪正式版发布），
 * 对应"最新正式版"一栏。
 */
const installLatestUpdate = async () => {
  installingUpdate.value = true;
  installProgress.value = "正在检测安装包…";
  let update: Update | null = null;
  try {
    update = await checkTauriUpdate();
  } catch (error) {
    installingUpdate.value = false;
    installProgress.value = "";
    message.error("检测安装包失败: " + error);
    return;
  }

  if (!update) {
    installingUpdate.value = false;
    installProgress.value = "";
    message.warning("未找到可自动安装的更新包，请前往 GitHub 手动下载");
    return;
  }

  let downloaded = 0;
  let total = 0;
  try {
    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case "Started":
          total = event.data.contentLength ?? 0;
          installProgress.value = total > 0 ? "下载中 0%" : "下载中…";
          break;
        case "Progress":
          downloaded += event.data.chunkLength;
          installProgress.value =
            total > 0 ? `下载中 ${Math.min(100, Math.round((downloaded / total) * 100))}%` : "下载中…";
          break;
        case "Finished":
          installProgress.value = "下载完成，正在安装…";
          break;
      }
    });
  } catch (error) {
    installingUpdate.value = false;
    installProgress.value = "";
    message.error("安装更新失败: " + error);
    return;
  }

  installProgress.value = "安装完成，即将重启应用…";
  message.success("更新安装完成，应用即将重启");
  setTimeout(() => {
    void relaunch();
  }, 1500);
};

/** 是否正在下载/安装 Beta 更新 */
const installingBetaUpdate = ref(false);

/** Beta 安装进度提示文案 */
const installBetaProgress = ref("");

/** 后端 check_and_install_beta_update 命令进度事件的取消监听函数 */
let unlistenBetaProgress: UnlistenFn | null = null;

interface BetaUpdateProgressEvent {
  status: "checking" | "downloading" | "finished";
  percent: number | null;
}

onMounted(async () => {
  unlistenBetaProgress = await listen<BetaUpdateProgressEvent>("beta-update-progress", (event) => {
    const { status, percent } = event.payload;
    if (status === "checking") {
      installBetaProgress.value = "正在检测安装包…";
    } else if (status === "downloading") {
      installBetaProgress.value = percent !== null ? `下载中 ${percent}%` : "下载中…";
    } else if (status === "finished") {
      installBetaProgress.value = "下载完成，正在安装…";
    }
  });
});

onBeforeUnmount(() => {
  unlistenBetaProgress?.();
});

/**
 * 检测到新 Beta 版后点击"立即更新并安装"：Beta 走独立于正式版的更新通道
 * （后端 `check_and_install_beta_update` 用 tauri-plugin-updater 的
 * `endpoints()` 显式指向 `updater-manifest-beta` 清单），因为 JS 侧的
 * `check()` 只认 tauri.conf.json 里的静态 endpoint 配置，无法在运行时切换。
 */
const installLatestBetaUpdate = async () => {
  installingBetaUpdate.value = true;
  installBetaProgress.value = "正在检测安装包…";
  try {
    const installedVersion = await invoke<string | null>("check_and_install_beta_update");
    if (!installedVersion) {
      message.warning("未找到可自动安装的 Beta 更新包，请前往 GitHub 手动下载");
      return;
    }
    message.success("Beta 更新安装完成，应用即将重启");
    setTimeout(() => {
      void relaunch();
    }, 1500);
  } catch (error) {
    message.error("安装 Beta 更新失败: " + error);
  } finally {
    installingBetaUpdate.value = false;
    installBetaProgress.value = "";
  }
};
</script>

<template>
  <!-- 关于卡片 -->
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
          <InformationCircleOutline />
        </n-icon>
        <span>关于</span>
      </div>
    </template>

    <!-- 关于内容 -->
    <div class="about-content">
      <div class="about-item">
        <span class="about-label">版本</span>
        <n-tag
          type="success"
          size="small"
        >
          v{{ currentAppVersion || "…" }}
        </n-tag>
      </div>
      <div class="about-item">
        <span class="about-label">许可证</span>
        <n-tag
          type="info"
          size="small"
        >
          MPL-2.0
        </n-tag>
      </div>
      <div class="about-item">
        <span class="about-label">GitHub</span>
        <n-text
          underline
          class="about-link"
        >
          baiyuheniao/BaiyuAISpace2
        </n-text>
      </div>
      
      <div
        class="about-item"
        style="margin-top: 16px;"
      >
        <n-space>
          <n-button
            type="primary"
            size="small"
            :loading="checkingUpdate"
            @click="checkLatestVersion"
          >
            检测最新版本
          </n-button>
          <n-button
            type="primary"
            size="small"
            @click="exportLogs"
          >
            导出日志
          </n-button>
        </n-space>
      </div>
    </div>
  </n-card>

  <!-- 检测最新版本弹窗 -->
  <n-modal
    v-model:show="showUpdateModal"
    title="版本检测结果"
    preset="card"
    style="width: 640px; max-width: 90vw;"
  >
    <div class="version-check-body">
      <div class="version-check-current">
        当前版本：<n-tag size="small">v{{ currentAppVersion }}</n-tag>
      </div>

      <template
        v-for="section in [
          { label: '最新正式版', info: latestReleases?.stable },
          { label: '最新 Beta 版', info: latestReleases?.beta },
        ]"
        :key="section.label"
      >
        <div class="version-check-section">
          <div class="version-check-section-header">
            <span class="version-check-section-label">{{ section.label }}</span>
            <template v-if="section.info">
              <n-tag size="small">v{{ section.info.version }}</n-tag>
              <n-tag
                size="small"
                :type="isNewerVersion(section.info.version) ? 'success' : 'default'"
              >
                {{ isNewerVersion(section.info.version) ? "有新版本" : "已是最新" }}
              </n-tag>
              <n-button
                text
                size="small"
                class="version-check-link"
                @click="openReleasePage(section.info.htmlUrl)"
              >
                在 GitHub 中查看
              </n-button>
              <n-button
                v-if="section.label === '最新正式版' && isNewerVersion(section.info.version)"
                size="small"
                :loading="installingUpdate"
                @click="installLatestUpdate"
              >
                立即更新并安装
              </n-button>
              <n-button
                v-if="section.label === '最新 Beta 版' && isNewerVersion(section.info.version)"
                size="small"
                :loading="installingBetaUpdate"
                @click="installLatestBetaUpdate"
              >
                立即更新并安装
              </n-button>
            </template>
          </div>

          <n-empty
            v-if="!section.info"
            description="GitHub 上暂无对应版本发布"
            size="small"
          />
          <div
            v-else
            class="version-check-notes"
            v-html="renderReleaseNotes(section.info.body)"
          />
          <div
            v-if="section.label === '最新正式版' && installingUpdate"
            class="version-check-progress"
          >
            {{ installProgress }}
          </div>
          <div
            v-if="section.label === '最新 Beta 版' && installingBetaUpdate"
            class="version-check-progress"
          >
            {{ installBetaProgress }}
          </div>
        </div>
      </template>
    </div>
  </n-modal>
</template>

<style scoped lang="scss">
/* 检测最新版本弹窗 */
.version-check-body {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.version-check-current {
  font-size: 14px;
  color: $ink-soft;
}

.version-check-section {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding-top: 16px;
  border-top: $border-faint;
}

.version-check-section-header {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.version-check-section-label {
  font-weight: 600;
}

.version-check-link {
  margin-left: auto;
}

.version-check-progress {
  font-size: 12px;
  color: $ink-faint;
  font-family: $font-mono;
}

.version-check-notes {
  font-size: 13px;
  line-height: $leading-body;
  color: $ink-soft;
  max-height: 260px;
  overflow-y: auto;

  :deep(h1),
  :deep(h2),
  :deep(h3) {
    font-size: 14px;
    font-weight: 600;
    color: $ink;
    margin: 8px 0 4px;
  }

  :deep(ul),
  :deep(ol) {
    padding-left: 20px;
  }

  :deep(a) {
    color: $ink;
  }

  :deep(code) {
    font-family: $font-mono;
    background: $surface;
    padding: 1px 4px;
  }
}
</style>
