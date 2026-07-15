// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// 通用错误分类：把英文/底层错误翻译成用户能看懂的中文提示。原本只在
// chat.ts 的聊天发送路径里用，现在提出来给 Docker/本地部署/LM Studio/MCP
// 等模块共用，避免各自裸抛原始错误。

/** 错误分类结果 */
export interface ClassifiedError {
  type: string;
  message: string;
}

/**
 * 把任意错误对象翻译成用户能看懂的中文提示。
 * 后端命令大多已经返回中文友好文案，这里主要兜底处理网络层
 * （fetch/超时/鉴权）等还没走到后端命令就失败的场景。
 */
export function classifyError(error: unknown): ClassifiedError {
  const errorStr = String(error);

  if (errorStr.includes("API key") || errorStr.includes("Unauthorized") || errorStr.includes("401")) {
    return { type: "auth", message: "API 密钥无效或已过期，请检查设置" };
  } else if (errorStr.includes("network") || errorStr.includes("Failed to fetch")) {
    return { type: "network", message: "网络连接错误，请检查网络设置" };
  } else if (errorStr.includes("timeout")) {
    return { type: "timeout", message: "请求超时，请重试或调整超时设置" };
  } else if (errorStr.includes("provider") || errorStr.includes("Invalid")) {
    return { type: "config", message: "API 配置错误，请检查服务商和模型" };
  } else {
    // 兜底：后端命令返回的错误现在基本已是中文友好文案，直接透出即可
    return { type: "unknown", message: errorStr };
  }
}
