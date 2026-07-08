// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// 应用内快捷键（如"新建会话"）的录制/匹配共用逻辑。区别于
// stores/settings.ts 里的托盘唤起快捷键——那个是要注册进操作系统的全局
// 快捷键（窗口最小化时也要生效），这里的都是纯前端 window keydown 监听，
// 只在应用窗口获得焦点时生效。

/** 键盘事件 code 中纯修饰键的集合——录制/匹配时都要跳过，等待真正的主键 */
const MODIFIER_CODES = new Set([
  "ControlLeft", "ControlRight",
  "AltLeft", "AltRight",
  "ShiftLeft", "ShiftRight",
  "MetaLeft", "MetaRight",
]);

/** 把 KeyboardEvent.code 转成更易读的主键名（KeyA -> A，Digit1 -> 1，其余原样） */
export function formatMainKey(code: string): string {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  return code;
}

/** 是否为纯修饰键（还未构成完整组合，调用方应继续等待下一次按键） */
export function isModifierOnly(e: KeyboardEvent): boolean {
  return MODIFIER_CODES.has(e.code);
}

/** 把按键事件格式化成和 accelerator 字符串一致的形式（如 "Ctrl+K"），
 * 没有搭配修饰键时返回 null（要求至少一个修饰键，避免和普通输入冲突）。 */
export function acceleratorFromEvent(e: KeyboardEvent): string | null {
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  if (mods.length === 0) return null;
  return [...mods, formatMainKey(e.code)].join("+");
}

/** 判断按键事件是否命中给定的 accelerator 字符串 */
export function eventMatchesAccelerator(e: KeyboardEvent, accelerator: string): boolean {
  if (isModifierOnly(e)) return false;
  return acceleratorFromEvent(e) === accelerator;
}
