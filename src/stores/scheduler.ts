import { defineStore } from 'pinia';
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type ScheduleKind = 'once' | 'interval' | 'daily' | 'weekly';

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
}

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

  async function toggleSchedule(id: string) {
    const updated = await invoke<Schedule>('schedule_toggle', { id });
    const idx = schedules.value.findIndex((s) => s.id === id);
    if (idx !== -1) schedules.value[idx] = updated;
  }

  // Listen for trigger events so UI can react (e.g. flash a badge)
  listen('scheduler://triggered', () => {
    // Reload to update lastRunAt / nextRunAt display
  });

  return { schedules, loadSchedules, createSchedule, deleteSchedule, toggleSchedule };
});
