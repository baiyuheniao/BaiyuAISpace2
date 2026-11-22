/* This Source Code Form is subject to the terms of the Mozilla Public
   - License, v. 2.0. If a copy of the MPL was not distributed with this
   - file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useSettingsStore } from "@/stores/settings";

/**
 * 根据配置 ID 获取 Embedding 配置名称
 *
 * @param configId - 配置 ID
 * @returns 配置名称 (格式: "名称 (模型名)" 或 "未知配置")
 */
export const getEmbeddingConfigName = (configId: string): string => {
  const config = useSettingsStore().embeddingApiConfigs.find(c => c.id === configId);
  return config ? `${config.name} (${config.model})` : "未知配置";
};
