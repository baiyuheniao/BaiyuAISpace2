// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use rusqlite::{Connection, params};
use super::types::{Schedule, ScheduleKind};

pub fn init_scheduler_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS schedules (
            id               TEXT PRIMARY KEY,
            name             TEXT NOT NULL,
            workspace_id     TEXT,
            target_agent_id  TEXT,
            message          TEXT NOT NULL,
            kind             TEXT NOT NULL,
            interval_minutes INTEGER,
            at_time          TEXT,
            weekday          INTEGER,
            once_at          INTEGER,
            next_run_at      INTEGER NOT NULL,
            last_run_at      INTEGER,
            enabled          INTEGER NOT NULL DEFAULT 1,
            created_at       INTEGER NOT NULL,
            updated_at       INTEGER NOT NULL
        )
        "#,
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_schedules_next_run ON schedules(next_run_at) WHERE enabled=1",
        [],
    )?;
    Ok(())
}

fn row_to_schedule(row: &rusqlite::Row<'_>) -> rusqlite::Result<Schedule> {
    Ok(Schedule {
        id:               row.get(0)?,
        name:             row.get(1)?,
        workspace_id:     row.get(2)?,
        target_agent_id:  row.get(3)?,
        message:          row.get(4)?,
        kind:             ScheduleKind::from_str(&row.get::<_, String>(5)?),
        interval_minutes: row.get(6)?,
        at_time:          row.get(7)?,
        weekday:          row.get(8)?,
        once_at:          row.get(9)?,
        next_run_at:      row.get(10)?,
        last_run_at:      row.get(11)?,
        enabled:          row.get::<_, i64>(12)? != 0,
        created_at:       row.get(13)?,
        updated_at:       row.get(14)?,
    })
}

pub fn insert_schedule(conn: &Connection, s: &Schedule) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"INSERT INTO schedules
           (id, name, workspace_id, target_agent_id, message, kind,
            interval_minutes, at_time, weekday, once_at,
            next_run_at, last_run_at, enabled, created_at, updated_at)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)"#,
        params![
            s.id, s.name, s.workspace_id, s.target_agent_id, s.message,
            s.kind.as_str(), s.interval_minutes, s.at_time, s.weekday, s.once_at,
            s.next_run_at, s.last_run_at, s.enabled as i64, s.created_at, s.updated_at
        ],
    )?;
    Ok(())
}

pub fn list_schedules(conn: &Connection, workspace_id: Option<&str>) -> Result<Vec<Schedule>, rusqlite::Error> {
    if let Some(wid) = workspace_id {
        let mut stmt = conn.prepare(
            "SELECT id,name,workspace_id,target_agent_id,message,kind,interval_minutes,at_time,weekday,once_at,next_run_at,last_run_at,enabled,created_at,updated_at FROM schedules WHERE workspace_id=?1 ORDER BY created_at DESC"
        )?;
        let result: rusqlite::Result<Vec<Schedule>> = stmt.query_map(params![wid], row_to_schedule)?.collect();
        return result;
    }
    let mut stmt = conn.prepare(
        "SELECT id,name,workspace_id,target_agent_id,message,kind,interval_minutes,at_time,weekday,once_at,next_run_at,last_run_at,enabled,created_at,updated_at FROM schedules ORDER BY created_at DESC"
    )?;
    let result: rusqlite::Result<Vec<Schedule>> = stmt.query_map([], row_to_schedule)?.collect();
    result
}

/// 返回所有已启用、且 `next_run_at` 早于或等于 `now_ms` 的定时任务。
pub fn list_due_schedules(conn: &Connection, now_ms: i64) -> Result<Vec<Schedule>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id,name,workspace_id,target_agent_id,message,kind,interval_minutes,at_time,weekday,once_at,next_run_at,last_run_at,enabled,created_at,updated_at FROM schedules WHERE enabled=1 AND next_run_at<=?1"
    )?;
    let result: rusqlite::Result<Vec<Schedule>> = stmt.query_map(params![now_ms], row_to_schedule)?.collect();
    result
}

pub fn delete_schedule(conn: &Connection, id: &str) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM schedules WHERE id=?1", params![id])?;
    Ok(())
}

pub fn get_schedule(conn: &Connection, id: &str) -> Result<Option<Schedule>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id,name,workspace_id,target_agent_id,message,kind,interval_minutes,at_time,weekday,once_at,next_run_at,last_run_at,enabled,created_at,updated_at FROM schedules WHERE id=?1"
    )?;
    let mut rows = stmt.query_map(params![id], row_to_schedule)?;
    Ok(rows.next().transpose()?)
}

/// 定时任务触发之后，更新 `next_run_at`、`last_run_at`，并可选择将其禁用。
pub fn update_after_fire(conn: &Connection, id: &str, next_run_at: Option<i64>, last_run_at: i64, disable: bool) -> Result<(), rusqlite::Error> {
    let now = chrono::Utc::now().timestamp_millis();
    if disable {
        conn.execute(
            "UPDATE schedules SET enabled=0, last_run_at=?1, next_run_at=COALESCE(?2, next_run_at), updated_at=?3 WHERE id=?4",
            params![last_run_at, next_run_at, now, id],
        )?;
    } else {
        conn.execute(
            "UPDATE schedules SET next_run_at=?1, last_run_at=?2, updated_at=?3 WHERE id=?4",
            params![next_run_at.unwrap_or(0), last_run_at, now, id],
        )?;
    }
    Ok(())
}

pub fn toggle_schedule(conn: &Connection, id: &str) -> Result<Option<Schedule>, rusqlite::Error> {
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "UPDATE schedules SET enabled=1-enabled, updated_at=?1 WHERE id=?2",
        params![now, id],
    )?;
    get_schedule(conn, id)
}
