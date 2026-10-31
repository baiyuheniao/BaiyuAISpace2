import { defineStore } from 'pinia';
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type ScheduleKind = 'once' | 'interval' | 'daily' | 'weekly' | 'condition';

/** 条件任务的脚本解释器。 */
export type CheckShell = 'powershell' | 'cmd' | 'sh' | 'python';

/** 条件任务的触发模式：edge＝条件由不成立变成立时触发一次；repeat＝成立期间按冷却时间重复触发。 */
export type TriggerMode = 'edge' | 'repeat';

export type CheckStatus = 'met' | 'not_met' | 'error';

export interface Schedule {
  id: string;
  name: string;
  workspaceId: string | null;
  targetAgentId: string | null;
  message: string;
  kind: ScheduleKind;
  intervalMinutes: number | null;
  atTime: string | null;
  weekday: number | null;
  onceAt: number | null;
  checkScript: string | null;
  checkShell: CheckShell | null;
  triggerMode: TriggerMode | null;
  cooldownMinutes: number | null;
  lastCheckAt: number | null;
  lastCheckStatus: CheckStatus | null;
  lastCheckOutput: string | null;
  lastConditionMet: boolean;
  consecutiveErrors: number;
  nextRunAt: number;
  lastRunAt: number | null;
  enabled: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface CreateScheduleRequest {
  name: string;
  workspaceId: string | null;
  targetAgentId: string | null;
  message: string;
  kind: ScheduleKind;
  intervalMinutes: number | null;
  atTime: string | null;
  weekday: number | null;
  onceAt: number | null;
  checkScript: string | null;
  checkShell: CheckShell | null;
  triggerMode: TriggerMode | null;
  cooldownMinutes: number | null;
}

/** 试运行检测脚本的结果。 */
export interface TestScriptResult {
  status: CheckStatus;
  output: string;
  exitCode: number | null;
  durationMs: number;
}

/** 后端每次检测完成后发出的事件。 */
export interface ScheduleCheckedEvent {
  scheduleId: string;
  scheduleName: string;
  workspaceId: string | null;
  status: CheckStatus;
  fired: boolean;
  autoDisabled: boolean;
}

/** 连续检测出错达到这个次数，后端会自动停用任务（与后端 MAX_CONSECUTIVE_CHECK_ERRORS 一致）。 */
export const MAX_CONSECUTIVE_CHECK_ERRORS = 5;

export const useSchedulerStore = defineStore('scheduler', () => {
  const schedules = ref<Schedule[]>([]);

  async function loadSchedules(workspaceId?: string) {
    schedules.value = await invoke<Schedule[]>('schedule_list', {
      workspaceId: workspaceId ?? null,
    });
  }

  async function createSchedule(req: CreateScheduleRequest): Promise<Schedule> {
    const created = await invoke<Schedule>('schedule_create', { request: req });
    schedules.value.unshift(created);
    return created;
  }

  async function deleteSchedule(id: string) {
    await invoke('schedule_delete', { id });
    schedules.value = schedules.value.filter((s) => s.id !== id);
  }

  /** 保存前试运行检测脚本：只跑一次并返回结果，不会触发任何任务。 */
  async function testScript(shell: CheckShell, script: string): Promise<TestScriptResult> {
    return await invoke<TestScriptResult>('schedule_test_script', { shell, script });
  }

  async function toggleSchedule(id: string) {
    const updated = await invoke<Schedule>('schedule_toggle', { id });
    const idx = schedules.value.findIndex((s) => s.id === id);
    if (idx !== -1) schedules.value[idx] = updated;
  }

  // Listen for trigger events so UI can react (e.g. flash a badge)
  listen('scheduler://triggered', () => {
    // Reload to update lastRunAt / nextRunAt display
  });

  return { schedules, loadSchedules, createSchedule, deleteSchedule, toggleSchedule, testScript };
});
