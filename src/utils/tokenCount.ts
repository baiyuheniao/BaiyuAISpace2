/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/**
 * Token 计数只能是跨模型近似值：OpenAI、Anthropic、Gemini 以及本地模型
 * 使用的 tokenizer 并不相同，前端也拿不到所有服务商的实际 usage。
 *
 * 这里按可见文本做稳定、即时的估算：
 * - 中日韩文字按每个字符约 1 token；
 * - 连续字母/数字按长度约每 4 个字符 1 token，短词至少 1 token；
 * - 标点、符号和 emoji 各按约 1 token。
 *
 * 图片、视频、隐藏系统提示词、RAG 上下文和工具调用上下文不计入。
 */

const CJK_CHAR =
  /[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uac00-\ud7af]/u;
const WORD_SEGMENT = /^[\p{L}\p{N}_]+$/u;
const TEXT_SEGMENTS =
  /[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uac00-\ud7af]|[\p{L}\p{N}_]+|[^\s]/gu;

export interface TokenCountableMessage {
  content: string;
}

export function estimateTokenCount(text: string): number {
  if (!text.trim()) return 0;

  const segments = text.normalize("NFC").match(TEXT_SEGMENTS) ?? [];
  return segments.reduce((total, segment) => {
    if (CJK_CHAR.test(segment)) return total + 1;
    if (WORD_SEGMENT.test(segment)) {
      return total + Math.max(1, Math.round(Array.from(segment).length / 4));
    }
    return total + 1;
  }, 0);
}

export function countMessageTokens(messages: readonly TokenCountableMessage[]): number {
  return messages.reduce((total, message) => total + estimateTokenCount(message.content), 0);
}

export function formatTokenCount(count: number): string {
  return Math.max(0, Math.round(count)).toLocaleString("zh-CN");
}
