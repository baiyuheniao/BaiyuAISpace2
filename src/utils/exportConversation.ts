/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/**
 * 对话导出
 * 把 ChatSession 转成用户可选的 JSON / TXT 文本，供 ChatView 落盘保存
 */

import type { ChatSession } from "@/stores/chat";

export type ExportFormat = "json" | "txt";

const formatTimestamp = (ts: number): string =>
  new Date(ts).toLocaleString("zh-CN", { hour12: false });

/** 文件名里不能出现的字符统一替换掉，避免不同平台的落盘校验各不相同 */
const sanitizeFilename = (name: string): string =>
  name.replace(/[\\/:*?"<>|]/g, "_").trim().slice(0, 40) || "对话";

/**
 * 构建导出内容
 *
 * @param session - 要导出的会话
 * @param format - 目标格式，"json" 保留结构化字段，"txt" 是人类可读的对话记录
 * @returns 文本内容 + 建议文件名（不含路径）
 */
export function buildConversationExport(
  session: ChatSession,
  format: ExportFormat
): { content: string; filename: string } {
  const dateStr = new Date(session.updatedAt).toISOString().slice(0, 10);
  const baseName = `${sanitizeFilename(session.title)}_${dateStr}`;

  // 流式中的占位消息不该出现在导出结果里
  const messages = session.messages.filter((m) => !m.streaming);

  if (format === "json") {
    const payload = {
      title: session.title,
      provider: session.provider,
      model: session.model,
      createdAt: session.createdAt,
      updatedAt: session.updatedAt,
      messages: messages.map((m) => ({
        role: m.role,
        content: m.content,
        timestamp: m.timestamp,
        error: m.error,
      })),
    };
    return {
      content: JSON.stringify(payload, null, 2),
      filename: `${baseName}.json`,
    };
  }

  const roleLabel = (role: string) =>
    role === "user" ? "你" : role === "assistant" ? "AI 助手" : "系统";

  const lines: string[] = [
    `对话标题：${session.title}`,
    `模型：${session.provider} / ${session.model}`,
    `创建时间：${formatTimestamp(session.createdAt)}`,
    `更新时间：${formatTimestamp(session.updatedAt)}`,
    "",
    "====================",
    "",
  ];

  for (const m of messages) {
    lines.push(`[${formatTimestamp(m.timestamp)}] ${roleLabel(m.role)}：`);
    lines.push(m.content || (m.error ? `（出错：${m.error}）` : "（空）"));
    lines.push("");
  }

  return {
    content: lines.join("\n"),
    filename: `${baseName}.txt`,
  };
}
