// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

/**
 * updater.ts - 启动时自动检测 GitHub Releases 上的新版本
 *
 * 流程：check() 向 tauri.conf.json 里配置的 endpoint（GitHub Releases 的
 * latest.json）查询是否有新版本 -> 有则在左下角弹出通知，用户确认后
 * downloadAndInstall() 下载已签名的安装包并静默运行安装程序（Windows 上
 * 直接原地升级安装目录，不会和旧版本并存）-> 安装完成后 relaunch() 重启。
 */

import { h } from "vue";
import { NButton, NSpace } from "naive-ui";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import type { useNotification } from "@/composables/useNotify";

type NotificationApi = ReturnType<typeof useNotification>;

export async function checkForAppUpdate(notification: NotificationApi): Promise<void> {
  let update;
  try {
    update = await check();
  } catch (err) {
    // 检测更新失败(比如断网)不打扰用户,只记日志
    console.error("检查更新失败", err);
    return;
  }
  if (!update) return;

  const n = notification.create({
    title: `发现新版本 ${update.version}`,
    description: update.body?.trim() || "建议更新到最新版本。",
    duration: 0,
    closable: true,
    action: () =>
      h(
        NSpace,
        { size: "small" },
        {
          default: () => [
            h(
              NButton,
              {
                size: "small",
                onClick: () => {
                  n.destroy();
                  void installUpdate(update, notification);
                },
              },
              { default: () => "立即更新" },
            ),
            h(
              NButton,
              { size: "small", quaternary: true, onClick: () => n.destroy() },
              { default: () => "以后再说" },
            ),
          ],
        },
      ),
  });
}

async function installUpdate(update: Update, notification: NotificationApi): Promise<void> {
  const progress = notification.create({
    title: "正在下载更新…",
    description: "准备下载",
    duration: 0,
    closable: false,
  });

  let downloaded = 0;
  let total = 0;
  try {
    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case "Started":
          total = event.data.contentLength ?? 0;
          progress.description = total > 0 ? "0%" : "下载中…";
          break;
        case "Progress":
          downloaded += event.data.chunkLength;
          progress.description = total > 0 ? `${Math.min(100, Math.round((downloaded / total) * 100))}%` : "下载中…";
          break;
        case "Finished":
          progress.description = "下载完成，正在安装…";
          break;
      }
    });
  } catch (err) {
    progress.destroy();
    notification.error({
      title: "更新失败",
      description: err instanceof Error ? err.message : String(err),
      duration: 4000,
    });
    return;
  }

  progress.destroy();
  notification.success({
    title: "更新完成",
    description: "即将重启应用以使用新版本",
    duration: 2000,
  });

  setTimeout(() => {
    void relaunch();
  }, 1500);
}
