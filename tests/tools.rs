use std::time::{SystemTime, UNIX_EPOCH};

use butterfly_bot::interfaces::plugins::Tool;
use butterfly_bot::tools::planning::PlanningTool;
use butterfly_bot::tools::reminders::RemindersTool;
use butterfly_bot::tools::tasks::TasksTool;
use butterfly_bot::tools::todo::TodoTool;
use butterfly_bot::tools::wakeup::WakeupTool;
use serde_json::json;
use tempfile::tempdir;

#[tokio::test]
async fn todo_tool_supports_bulk_create_reorder_and_complete() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("todo.db");
    let path = db_path.to_string_lossy().to_string();

    let tool = TodoTool::new();
    tool.configure(&json!({"tools": {"todo": {"sqlite_path": path}}}))
        .expect("configure todo tool");

    let created = tool
        .execute(json!({
            "action": "add_many",
            "user_id": "u1",
            "items": [
                "draft release notes",
                {"title": "ship patch", "notes": "before noon"}
            ]
        }))
        .await
        .expect("create many");

    let ids: Vec<i64> = created["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["id"].as_i64().expect("item id"))
        .collect();
    assert_eq!(ids.len(), 2);

    tool.execute(json!({
        "action": "reorder",
        "user_id": "u1",
        "ordered_ids": [ids[1], ids[0]]
    }))
    .await
    .expect("reorder");

    let listed = tool
        .execute(json!({"action": "list", "user_id": "u1", "status": "open"}))
        .await
        .expect("list open");
    let listed_ids: Vec<i64> = listed["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["id"].as_i64().expect("list id"))
        .collect();
    assert_eq!(listed_ids, vec![ids[1], ids[0]]);

    tool.execute(json!({"action": "complete", "user_id": "u1", "id": ids[1]}))
        .await
        .expect("complete item");

    let completed = tool
        .execute(json!({"action": "list", "user_id": "u1", "status": "completed"}))
        .await
        .expect("list completed");
    assert_eq!(
        completed["items"]
            .as_array()
            .expect("completed items")
            .len(),
        1
    );
}

#[tokio::test]
async fn tasks_tool_schedules_and_toggles_task() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("tasks.db");
    let path = db_path.to_string_lossy().to_string();

    let tool = TasksTool::new();
    tool.configure(&json!({"tools": {"tasks": {"sqlite_path": path}}}))
        .expect("configure tasks tool");

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("unix time")
        .as_secs() as i64;

    let scheduled = tool
        .execute(json!({
            "action": "schedule",
            "user_id": "u1",
            "name": "daily sync",
            "prompt": "summarize changes",
            "run_at": now + 120,
            "interval_minutes": 30
        }))
        .await
        .expect("schedule task");
    let id = scheduled["task"]["id"].as_i64().expect("task id");
    assert_eq!(scheduled["task"]["enabled"], json!(true));

    tool.execute(json!({"action": "cancel", "user_id": "u1", "id": id}))
        .await
        .expect("cancel task");
    let disabled = tool
        .execute(json!({"action": "list", "user_id": "u1", "status": "disabled"}))
        .await
        .expect("list disabled tasks");
    assert_eq!(disabled["tasks"].as_array().expect("tasks").len(), 1);

    tool.execute(json!({"action": "enable", "user_id": "u1", "id": id}))
        .await
        .expect("enable task");
    let enabled = tool
        .execute(json!({"action": "list", "user_id": "u1", "status": "enabled"}))
        .await
        .expect("list enabled tasks");
    assert_eq!(enabled["tasks"].as_array().expect("tasks").len(), 1);

    let deleted = tool
        .execute(json!({"action": "delete", "user_id": "u1", "id": id}))
        .await
        .expect("delete task");
    assert_eq!(deleted["deleted"], json!(true));
}

#[tokio::test]
async fn planning_tool_crud_flow_works() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("plans.db");
    let path = db_path.to_string_lossy().to_string();

    let tool = PlanningTool::new();
    tool.configure(&json!({"tools": {"planning": {"sqlite_path": path}}}))
        .expect("configure planning tool");

    let created = tool
        .execute(json!({
            "action": "create",
            "user_id": "u1",
            "title": "Ship v1",
            "goal": "Release with docs",
            "steps": ["freeze scope", "publish changelog"],
            "status": "draft"
        }))
        .await
        .expect("create plan");
    let id = created["plan"]["id"].as_i64().expect("plan id");

    let fetched = tool
        .execute(json!({"action": "get", "user_id": "u1", "id": id}))
        .await
        .expect("get plan");
    assert_eq!(fetched["plan"]["title"], json!("Ship v1"));

    let updated = tool
        .execute(json!({
            "action": "update",
            "user_id": "u1",
            "id": id,
            "status": "active",
            "steps": ["freeze scope", "publish changelog", "announce release"]
        }))
        .await
        .expect("update plan");
    assert_eq!(updated["plan"]["status"], json!("active"));

    let listed = tool
        .execute(json!({"action": "list", "user_id": "u1"}))
        .await
        .expect("list plans");
    assert_eq!(listed["plans"].as_array().expect("plans").len(), 1);

    let deleted = tool
        .execute(json!({"action": "delete", "user_id": "u1", "id": id}))
        .await
        .expect("delete plan");
    assert_eq!(deleted["deleted"], json!(true));
}

#[tokio::test]
async fn reminders_tool_handles_aliases_and_lifecycle() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("reminders.db");
    let path = db_path.to_string_lossy().to_string();

    let tool = RemindersTool::new();
    tool.configure(&json!({"tools": {"reminders": {"sqlite_path": path}}}))
        .expect("configure reminders tool");

    let created = tool
        .execute(json!({
            "action": "set",
            "user_id": "u1",
            "title": "Stand up",
            "in_seconds": 60
        }))
        .await
        .expect("create reminder via alias");
    let id = created["reminder"]["id"].as_i64().expect("reminder id");

    let snoozed = tool
        .execute(json!({
            "action": "snooze",
            "user_id": "u1",
            "id": id,
            "delay_seconds": 120
        }))
        .await
        .expect("snooze reminder");
    assert_eq!(snoozed["snoozed"], json!(true));

    let completed = tool
        .execute(json!({"action": "done", "user_id": "u1", "id": id}))
        .await
        .expect("complete reminder via alias");
    assert_eq!(completed["completed"], json!(true));

    let listed = tool
        .execute(json!({"action": "show", "user_id": "u1", "status": "completed"}))
        .await
        .expect("list reminders via alias");
    assert_eq!(listed["reminders"].as_array().expect("reminders").len(), 1);

    let cleared = tool
        .execute(json!({"action": "clear_all", "user_id": "u1", "status": "all"}))
        .await
        .expect("clear reminders via alias");
    assert_eq!(cleared["deleted"], json!(1));
}

#[tokio::test]
async fn wakeup_tool_create_toggle_and_delete() {
    let dir = tempdir().expect("temp dir");
    let db_path = dir.path().join("wakeup.db");
    let path = db_path.to_string_lossy().to_string();

    let tool = WakeupTool::new();
    tool.configure(&json!({"tools": {"wakeup": {"sqlite_path": path}}}))
        .expect("configure wakeup tool");

    let created = tool
        .execute(json!({
            "action": "create",
            "user_id": "u1",
            "name": "Morning review",
            "prompt": "summarize priorities",
            "interval_minutes": 15
        }))
        .await
        .expect("create wakeup task");
    let id = created["task"]["id"].as_i64().expect("wakeup id");

    tool.execute(json!({"action": "disable", "user_id": "u1", "id": id}))
        .await
        .expect("disable wakeup task");
    let disabled = tool
        .execute(json!({"action": "list", "user_id": "u1", "status": "disabled"}))
        .await
        .expect("list disabled wakeup tasks");
    assert_eq!(disabled["tasks"].as_array().expect("tasks").len(), 1);

    tool.execute(json!({"action": "enable", "user_id": "u1", "id": id}))
        .await
        .expect("enable wakeup task");
    let enabled = tool
        .execute(json!({"action": "list", "user_id": "u1", "status": "enabled"}))
        .await
        .expect("list enabled wakeup tasks");
    assert_eq!(enabled["tasks"].as_array().expect("tasks").len(), 1);

    let deleted = tool
        .execute(json!({"action": "delete", "user_id": "u1", "id": id}))
        .await
        .expect("delete wakeup task");
    assert_eq!(deleted["deleted"], json!(true));
}
