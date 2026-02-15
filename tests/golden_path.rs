use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::tempdir;

use butterfly_bot::planning::PlanStore;
use butterfly_bot::reminders::ReminderStore;
use butterfly_bot::tasks::{TaskStatus, TaskStore};

#[tokio::test]
async fn golden_path_plan_create_and_update() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("golden-path.db");
    let db_path = db_path.to_string_lossy().to_string();
    let user_id = "golden-user";

    let plans = PlanStore::new(&db_path).await.unwrap();
    let initial_steps = json!([
        {"title": "capture intent", "status": "todo"},
        {"title": "schedule execution", "status": "todo"}
    ]);

    let created = plans
        .create_plan(
            user_id,
            "Morning ops",
            "Complete plan-task-reminder loop",
            Some(&initial_steps),
            Some("draft"),
        )
        .await
        .unwrap();

    assert_eq!(created.user_id, user_id);
    assert_eq!(created.status, "draft");

    let updated_steps = json!([
        {"title": "capture intent", "status": "done"},
        {"title": "schedule execution", "status": "done"},
        {"title": "verify reminders", "status": "todo"}
    ]);
    let updated = plans
        .update_plan(created.id, None, None, Some(&updated_steps), Some("active"))
        .await
        .unwrap();

    assert_eq!(updated.status, "active");
    assert_eq!(updated.steps, Some(updated_steps.clone()));

    let listed = plans.list_plans(user_id, 10).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);
}

#[tokio::test]
async fn golden_path_due_task_runs_and_completes() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("golden-path.db");
    let db_path = db_path.to_string_lossy().to_string();
    let user_id = "golden-user";
    let now = unix_now();

    let tasks = TaskStore::new(&db_path).await.unwrap();
    let task = tasks
        .create_task(
            user_id,
            "Run plan follow-up",
            "Generate progress note",
            now - 30,
            None,
        )
        .await
        .unwrap();

    let due_check = unix_now() + 2;
    let due = tasks.list_due(due_check, 10).await.unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id, task.id);

    tasks.complete_one_shot(task.id).await.unwrap();

    let due_after = tasks.list_due(due_check + 60, 10).await.unwrap();
    assert!(due_after.is_empty());

    let all = tasks
        .list_tasks(user_id, TaskStatus::All, 10)
        .await
        .unwrap();
    assert_eq!(all.len(), 1);
    assert!(!all[0].enabled);
}

#[tokio::test]
async fn golden_path_due_reminder_fires_once() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("golden-path.db");
    let db_path = db_path.to_string_lossy().to_string();
    let user_id = "golden-user";
    let now = 1_730_000_000_i64;

    let reminders = ReminderStore::new(&db_path).await.unwrap();
    let reminder = reminders
        .create_reminder(user_id, "Standup follow-up", now - 10)
        .await
        .unwrap();

    let peeked = reminders
        .peek_due_reminders(user_id, now, 10)
        .await
        .unwrap();
    assert_eq!(peeked.len(), 1);
    assert_eq!(peeked[0].id, reminder.id);

    let fired = reminders.due_reminders(user_id, now, 10).await.unwrap();
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].id, reminder.id);

    let fired_again = reminders.due_reminders(user_id, now + 1, 10).await.unwrap();
    assert!(fired_again.is_empty());
}

#[tokio::test]
async fn golden_path_persists_across_store_restart() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("golden-path.db");
    let db_path = db_path.to_string_lossy().to_string();
    let user_id = "golden-user";
    let now = unix_now();

    {
        let plans = PlanStore::new(&db_path).await.unwrap();
        let tasks = TaskStore::new(&db_path).await.unwrap();
        let reminders = ReminderStore::new(&db_path).await.unwrap();

        let _ = plans
            .create_plan(
                user_id,
                "Daily loop",
                "Persist through restart",
                None,
                Some("draft"),
            )
            .await
            .unwrap();
        let _ = tasks
            .create_task(user_id, "Restart check task", "noop", now - 5, None)
            .await
            .unwrap();
        let _ = reminders
            .create_reminder(user_id, "Restart check reminder", now - 5)
            .await
            .unwrap();
    }

    let plans_after = PlanStore::new(&db_path).await.unwrap();
    let tasks_after = TaskStore::new(&db_path).await.unwrap();
    let reminders_after = ReminderStore::new(&db_path).await.unwrap();

    let plan_items = plans_after.list_plans(user_id, 10).await.unwrap();
    assert_eq!(plan_items.len(), 1);

    let due_tasks = tasks_after.list_due(unix_now() + 2, 10).await.unwrap();
    assert_eq!(due_tasks.len(), 1);

    let due_reminders = reminders_after
        .due_reminders(user_id, now, 10)
        .await
        .unwrap();
    assert_eq!(due_reminders.len(), 1);
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
