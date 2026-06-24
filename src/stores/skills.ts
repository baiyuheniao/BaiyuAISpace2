/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/**
 * Skill (技能) Store
 *
 * Skill 是用户自定义的可复用能力包：一段指令文本 + 可选绑定的 MCP 工具 +
 * 可选的资源文件（类似 Claude Agent Skills 里 SKILL.md 同目录下的辅助文件）。
 * 在 Chat 模块里可以手动选择激活，也可以让模型根据名称/描述自主判断调用。
 */

import { ref, computed } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";

/**
 * Skill 配置
 * 与后端 Skill 结构对应
 */
export interface Skill {
  id: string;
  name: string;
  description: string;
  instructions: string;
  boundMcpServerIds: string[];
  enabled: boolean;
  resourceFiles: string[];
  createdAt: number;
  updatedAt: number;
}

/**
 * 创建/更新 Skill 时使用的可编辑字段
 */
export type SkillDraft = Omit<Skill, "createdAt" | "updatedAt">;

export const useSkillsStore = defineStore("skills", () => {
  // ============ 响应式状态 ============

  const skills = ref<Skill[]>([]);
  const isLoading = ref(false);

  // ============ 计算属性 ============

  const enabledSkills = computed(() => skills.value.filter((s) => s.enabled));

  // ============ 方法函数 ============

  const loadSkills = async () => {
    isLoading.value = true;
    try {
      skills.value = await invoke<Skill[]>("list_skills");
    } catch (error) {
      console.error("Failed to load skills:", error);
      skills.value = [];
    } finally {
      isLoading.value = false;
    }
  };

  /**
   * 创建或更新 Skill (draft.id 为空字符串时为创建)
   */
  const saveSkill = async (draft: SkillDraft): Promise<Skill | null> => {
    try {
      const saved = await invoke<Skill>("save_skill", {
        skill: {
          ...draft,
          createdAt: 0,
          updatedAt: 0,
        },
      });
      const idx = skills.value.findIndex((s) => s.id === saved.id);
      if (idx !== -1) {
        skills.value[idx] = saved;
      } else {
        skills.value.unshift(saved);
      }
      return saved;
    } catch (error) {
      console.error("Failed to save skill:", error);
      return null;
    }
  };

  const deleteSkill = async (skillId: string): Promise<boolean> => {
    try {
      await invoke("delete_skill", { skillId });
      skills.value = skills.value.filter((s) => s.id !== skillId);
      return true;
    } catch (error) {
      console.error("Failed to delete skill:", error);
      return false;
    }
  };

  const toggleSkillEnabled = async (skill: Skill): Promise<void> => {
    await saveSkill({ ...skill, enabled: !skill.enabled });
  };

  /**
   * 添加资源文件 (filePath 是通过文件选择器选中的绝对路径)
   */
  const addResourceFile = async (skillId: string, filePath: string): Promise<Skill | null> => {
    try {
      const updated = await invoke<Skill>("add_skill_resource_file", {
        skillId,
        filePath,
      });
      const idx = skills.value.findIndex((s) => s.id === updated.id);
      if (idx !== -1) skills.value[idx] = updated;
      return updated;
    } catch (error) {
      console.error("Failed to add resource file:", error);
      return null;
    }
  };

  const removeResourceFile = async (skillId: string, filename: string): Promise<Skill | null> => {
    try {
      const updated = await invoke<Skill>("remove_skill_resource_file", {
        skillId,
        filename,
      });
      const idx = skills.value.findIndex((s) => s.id === updated.id);
      if (idx !== -1) skills.value[idx] = updated;
      return updated;
    } catch (error) {
      console.error("Failed to remove resource file:", error);
      return null;
    }
  };

  const readResourceFile = async (skillId: string, filename: string): Promise<string | null> => {
    try {
      return await invoke<string>("read_skill_resource_file", { skillId, filename });
    } catch (error) {
      console.error("Failed to read resource file:", error);
      return null;
    }
  };

  return {
    // 状态
    skills,
    isLoading,

    // 计算属性
    enabledSkills,

    // 方法
    loadSkills,
    saveSkill,
    deleteSkill,
    toggleSkillEnabled,
    addResourceFile,
    removeResourceFile,
    readResourceFile,
  };
});
