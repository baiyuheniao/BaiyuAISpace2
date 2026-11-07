// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use rusqlite::{Connection, params};
use super::types::{Schedule, ScheduleKind};

/// 所有查询共用的列清单（顺序必须和 `row_to_schedule` 一致）。
const SCHEDULE_COLUMNS: &str = "id,name,workspace_id,target_agent_id,message,kind,interval_minutes,at_time,weekday,once_at,next_run_at,last_run_at,enabled,created_at,updated_at,check_script,check_shell,trigger_mode,cooldown_minutes,last_check_at,last_check_status,last_check_output,last_condition_met,consecutive_errors";

/// 条件任务新增的列。旧库没有，启动时按需补上；已有任务这些列为 NULL/默认值。
const CONDITION_COLUMNS: [(&str, &str); 8] = [
    ("check_script", "TEXT"),
    ("check_shell", "TEXT"),
    ("trigger_mode", "TEXT"),
    ("cooldown_minutes", "INTEGER"),
    ("last_check_at", "INTEGER"),
    ("last_check_status", "TEXT"),
    ("last_check_output", "TEXT"),
    ("last_condition_met", "INTEGER NOT NULL DEFAULT 0"),
];

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
    migrate_schedules_table(conn)?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_schedules_next_run ON schedules(next_run_at) WHERE enabled=1",
        [],
    )?;
    Ok(())
}

fn has_column(conn: &Connection, column: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM pragma_table_info('schedules') WHERE name = ?1",
        params![column],
        |_| Ok(true),
    )
    .unwrap_or(false)
}

fn migrate_schedules_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    for (column, definition) in CONDITION_COLUMNS {
        if !has_column(conn, column) {
            conn.execute(&format!("ALTER TABLE schedules ADD COLUMN {column} {definition}"), [])?;
        }
    }
    if !has_column(conn, "consecutive_errors") {
        conn.execute("ALTER TABLE schedules ADD COLUMN consecutive_errors INTEGER NOT NULL DEFAULT 0", [])?;
    }
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
        check_script:     row.get(15)?,
        check_shell:      row.get(16)?,
        trigger_mode:     row.get(17)?,
        cooldown_minutes: row.get(18)?,
        last_check_at:    row.get(19)?,
        last_check_status: row.get(20)?,
        last_check_output: row.get(21)?,
        last_condition_met: row.get::<_, i64>(22)? != 0,
        consecutive_errors: row.get(23)?,
    })
}

pub fn insert_schedule(conn: &Connection, s: &Schedule) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"INSERT INTO schedules
           (id, name, workspace_id, target_agent_id, message, kind,
            interval_minutes, at_time, weekday, once_at,
            next_run_at, last_run_at, enabled, created_at, updated_at,
            check_script, check_shell, trigger_mode, cooldown_minutes)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)"#,
        params![
            s.id, s.name, s.workspace_id, s.target_agent_id, s.message,
            s.kind.as_str(), s.interval_minutes, s.at_time, s.weekday, s.once_at,
            s.next_run_at, s.last_run_at, s.enabled as i64, s.created_at, s.updated_at,
            s.check_script, s.check_shell, s.trigger_mode, s.cooldown_minutes
        ],
    )?;
    Ok(())
}

pub fn list_schedules(conn: &Connection, workspace_id: Option<&str>) -> Result<Vec<Schedule>, rusqlite::Error> {
    if let Some(wid) = workspace_id {
        let mut stmt = conn.prepare(
            &format!("SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE workspace_id=?1 ORDER BY created_at DESC")
        )?;
        let result: rusqlite::Result<Vec<Schedule>> = stmt.query_map(params![wid], row_to_schedule)?.collect();
        return result;
    }
    let mut stmt = conn.prepare(
        &format!("SELECT {SCHEDULE_COLUMNS} FROM schedules ORDER BY created_at DESC")
    )?;
    let result: rusqlite::Result<Vec<Schedule>> = stmt.query_map([], row_to_schedule)?.collect();
    result
}

/// 返回所有已启用、且 `next_run_at` 早于或等于 `now_ms` 的定时任务。
pub fn list_due_schedules(conn: &Connection, now_ms: i64) -> Result<Vec<Schedule>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        &format!("SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE enabled=1 AND next_run_at<=?1")
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
        &format!("SELECT {SCHEDULE_COLUMNS} FROM schedules WHERE id=?1")
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
        "UPDATE schedules SET enabled=1-enabled, consecutive_errors=0, updated_at=?1 WHERE id=?2",
        params![now, id],
    )?;
    get_schedule(conn, id)
}

/// 一次条件检测的落库内容。
pub struct CheckUpdate<'a> {
    pub next_run_at: i64,
    pub checked_at: i64,
    /// `met` / `not_met` / `error`
    pub status: &'a str,
    pub output: &'a str,
    /// 有明确结论时更新"上次条件是否成立"；出错时为 `None`，保持原值。
    pub condition_met: Option<bool>,
    pub consecutive_errors: i64,
    /// 这次检测触发了任务：同时更新 `last_run_at`。
    pub fired: bool,
    /// 连续出错达到上限：停用任务。
    pub disable: bool,
}

pub fn update_after_check(conn: &Connection, id: &str, u: &CheckUpdate<'_>) -> Result<(), rusqlite::Error> {
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        r#"UPDATE schedules SET
               next_run_at = ?1,
               last_check_at = ?2,
               last_check_status = ?3,
               last_check_output = ?4,
               last_condition_met = COALESCE(?5, last_condition_met),
               consecutive_errors = ?6,
               last_run_at = CASE WHEN ?7 THEN ?2 ELSE last_run_at END,
               enabled = CASE WHEN ?8 THEN 0 ELSE enabled END,
               updated_at = ?9
           WHERE id = ?10"#,
        params![
            u.next_run_at, u.checked_at, u.status, u.output,
            u.condition_met.map(|m| m as i64), u.consecutive_errors,
            u.fired, u.disable, now, id
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn condition(id: &str) -> Schedule {
        Schedule {
            id: id.into(), name: "磁盘检测".into(), workspace_id: Some("w1".into()), target_agent_id: None,
            message: "磁盘快满了".into(), kind: ScheduleKind::Condition,
            interval_minutes: Some(5), at_time: None, weekday: None, once_at: None,
            check_script: Some("df -h".into()), check_shell: Some("sh".into()),
            trigger_mode: Some("repeat".into()), cooldown_minutes: Some(30),
            last_check_at: None, last_check_status: None, last_check_output: None,
            last_condition_met: false, consecutive_errors: 0,
            next_run_at: 100, last_run_at: None, enabled: true, created_at: 1, updated_at: 1,
        }
    }

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_scheduler_tables(&conn).unwrap();
        conn
    }

    #[test]
    fn condition_fields_round_trip() {
        let conn = conn();
        insert_schedule(&conn, &condition("s1")).unwrap();
        let loaded = get_schedule(&conn, "s1").unwrap().unwrap();
        assert_eq!(loaded.kind, ScheduleKind::Condition);
        assert_eq!(loaded.check_script.as_deref(), Some("df -h"));
        assert_eq!(loaded.check_shell.as_deref(), Some("sh"));
        assert_eq!(loaded.trigger_mode.as_deref(), Some("repeat"));
        assert_eq!(loaded.cooldown_minutes, Some(30));
        assert_eq!(list_schedules(&conn, Some("w1")).unwrap().len(), 1);
    }

    #[test]
    fn legacy_schedules_table_is_migrated_and_existing_rows_survive() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE schedules (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, workspace_id TEXT, target_agent_id TEXT,
                message TEXT NOT NULL, kind TEXT NOT NULL, interval_minutes INTEGER, at_time TEXT,
                weekday INTEGER, once_at INTEGER, next_run_at INTEGER NOT NULL, last_run_at INTEGER,
                enabled INTEGER NOT NULL DEFAULT 1, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
            );
            INSERT INTO schedules (id,name,message,kind,interval_minutes,next_run_at,created_at,updated_at)
            VALUES ('old','每小时提醒','看看进展','interval',60,500,1,1);
            "#,
        ).unwrap();

        init_scheduler_tables(&conn).expect("迁移旧库不应失败");
        init_scheduler_tables(&conn).expect("迁移必须可重复执行");

        let old = get_schedule(&conn, "old").unwrap().unwrap();
        assert_eq!(old.kind, ScheduleKind::Interval);
        assert_eq!(old.message, "看看进展");
        assert!(old.check_script.is_none());
        assert_eq!(old.consecutive_errors, 0);
        assert!(!old.last_condition_met);
    }

    #[test]
    fn a_successful_fired_check_updates_state_and_last_run() {
        let conn = conn();
        insert_schedule(&conn, &condition("s1")).unwrap();
        update_after_check(&conn, "s1", &CheckUpdate {
            next_run_at: 900, checked_at: 600, status: "met", output: "C: 95%",
            condition_met: Some(true), consecutive_errors: 0, fired: true, disable: false,
        }).unwrap();

        let s = get_schedule(&conn, "s1").unwrap().unwrap();
        assert_eq!(s.next_run_at, 900);
        assert_eq!(s.last_check_at, Some(600));
        assert_eq!(s.last_check_status.as_deref(), Some("met"));
        assert_eq!(s.last_check_output.as_deref(), Some("C: 95%"));
        assert!(s.last_condition_met);
        assert_eq!(s.last_run_at, Some(600), "触发了才更新 last_run_at");
        assert!(s.enabled);
    }

    #[test]
    fn an_error_check_keeps_the_last_condition_and_can_disable_the_task() {
        let conn = conn();
        let mut base = condition("s1");
        base.last_condition_met = true;
        insert_schedule(&conn, &base).unwrap();
        conn.execute("UPDATE schedules SET last_condition_met=1 WHERE id='s1'", []).unwrap();

        update_after_check(&conn, "s1", &CheckUpdate {
            next_run_at: 900, checked_at: 600, status: "error", output: "无法启动 sh",
            condition_met: None, consecutive_errors: 5, fired: false, disable: true,
        }).unwrap();

        let s = get_schedule(&conn, "s1").unwrap().unwrap();
        assert!(s.last_condition_met, "出错不能把上次的结论改掉");
        assert_eq!(s.last_run_at, None, "没触发不能更新 last_run_at");
        assert_eq!(s.consecutive_errors, 5);
        assert!(!s.enabled);

        // 重新启用会把错误计数清零
        let toggled = toggle_schedule(&conn, "s1").unwrap().unwrap();
        assert!(toggled.enabled);
        assert_eq!(toggled.consecutive_errors, 0);
    }
}
