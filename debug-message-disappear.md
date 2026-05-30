# Debug Session: message-disappear

## Status: [OPEN - FIX APPLIED]

## Bug Description
模型输出完成后消息消失，历史记录无法正常显示。

## Root Cause (CONFIRMED)

**`save_session` 使用 `INSERT OR REPLACE` + `ON DELETE CASCADE` 级联删除消息**

### 触发链路：
```
1. saveMessageToDb(user) → DB: 1条消息 ✓
2. saveSessionToDb() → INSERT OR REPLACE → DELETE旧session → CASCADE删除所有messages → DB: 0条 ❌
3. saveMessageToDb(assistant) → DB: 1条消息 (只有assistant)
4. saveSessionToDb() → INSERT OR REPLACE → DELETE旧session → CASCADE删除所有messages → DB: 0条 ❌
5. loadSessionsFromDb() → get_messages → 0条消息 → UI显示空 ❌
```

### 日志证据：
```
[20:56:40] save_message(user) → VERIFY: 1 message ✓
[20:56:40] Session saved → INSERT OR REPLACE → CASCADE删除user消息！
[20:56:47] save_message(assistant) → VERIFY: 1 message (只有assistant，user已被删)
[20:56:47] Session saved → INSERT OR REPLACE → CASCADE删除assistant消息！
[20:56:47] get_messages → 0 messages ❌
```

### 表结构关键约束：
```sql
-- messages 表有外键级联删除
FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
```

### `INSERT OR REPLACE` 在 SQLite 中的行为：
1. 如果主键已存在 → **先 DELETE 旧行**
2. **ON DELETE CASCADE 触发 → 删除所有关联 messages**
3. 然后 INSERT 新行

## Fix Applied

### 修改1: `save_session` - 使用 `ON CONFLICT` 替代 `INSERT OR REPLACE`
```sql
-- Before (BUG):
INSERT OR REPLACE INTO sessions (...) VALUES (...)

-- After (FIX):
INSERT INTO sessions (...) VALUES (...)
ON CONFLICT(id) DO UPDATE SET
    title = excluded.title,
    provider = excluded.provider,
    model = excluded.model,
    api_config_id = excluded.api_config_id,
    updated_at = excluded.updated_at
```

### 修改2: `save_message` - 同样使用 `ON CONFLICT`
```sql
-- Before:
INSERT OR REPLACE INTO messages (...) VALUES (...)

-- After:
INSERT INTO messages (...) VALUES (...)
ON CONFLICT(id) DO UPDATE SET
    content = excluded.content,
    error = excluded.error
```

## Files Changed
- `src-tauri/src/db.rs`: save_session 和 save_message 函数

## Pending Verification
- [ ] 重新编译并测试消息是否正常保存和显示
