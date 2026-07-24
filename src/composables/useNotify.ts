// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// 对 naive-ui 的 useMessage / useNotification 做一层薄封装。报错弹窗始终显示，
// 是否播放声音由设置里的敏感度和调用方标注的错误等级共同决定。

import { useMessage as useNaiveMessage, useNotification as useNaiveNotification } from "naive-ui";
import { useSettingsStore, type ErrorSoundLevel } from "@/stores/settings";
import { playErrorSound } from "@/utils/sound";

export type ErrorSoundSeverity = "normal" | "critical";

const shouldPlayErrorSound = (
  level: ErrorSoundLevel,
  severity: ErrorSoundSeverity,
): boolean => level === "all" || (level === "critical" && severity === "critical");

export function useMessage(severity: ErrorSoundSeverity = "normal") {
  const inner = useNaiveMessage();
  const settings = useSettingsStore();
  return {
    ...inner,
    error: (...args: Parameters<typeof inner.error>) => {
      if (shouldPlayErrorSound(settings.errorSoundLevel, severity)) playErrorSound();
      return inner.error(...args);
    },
  };
}

export function useNotification(severity: ErrorSoundSeverity = "normal") {
  const inner = useNaiveNotification();
  const settings = useSettingsStore();
  return {
    ...inner,
    error: (...args: Parameters<typeof inner.error>) => {
      if (shouldPlayErrorSound(settings.errorSoundLevel, severity)) playErrorSound();
      return inner.error(...args);
    },
  };
}
