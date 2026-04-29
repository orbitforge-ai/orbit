use orbit_engine::db::repos::postgres::PgRepos;
use orbit_engine::db::repos::{ChatSessionListFilter, Repos};
use orbit_engine::models::agent::CreateAgent;
use orbit_engine::models::bus::CreateBusSubscription;
use orbit_engine::models::project::CreateProject;
use orbit_engine::models::project_board::CreateProjectBoard;
use orbit_engine::models::project_workflow::{CreateProjectWorkflow, WorkflowGraph};
use orbit_engine::models::schedule::CreateSchedule;
use orbit_engine::models::task::CreateTask;
use orbit_engine::models::work_item::CreateWorkItem;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

#[tokio::test]
#[ignore = "requires ORBIT_TEST_POSTGRES_URL pointing at a migrated Postgres test database"]
async fn pg_repos_scope_command_surface_by_tenant() -> Result<(), String> {
    let url = std::env::var("ORBIT_TEST_POSTGRES_URL").map_err(|err| err.to_string())?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .map_err(|err| err.to_string())?;

    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let tenant_a = format!("b5_rls_a_{suffix}");
    let tenant_b = format!("b5_rls_b_{suffix}");
    cleanup_tenant(&pool, &tenant_a).await;
    cleanup_tenant(&pool, &tenant_b).await;

    let a = PgRepos::with_tenant(pool.clone(), tenant_a.clone());
    let b = PgRepos::with_tenant(pool.clone(), tenant_b.clone());

    let agent_a = a
        .agents()
        .create_basic(CreateAgent {
            name: format!("B5 Agent A {suffix}"),
            description: Some("tenant A".to_string()),
            max_concurrent_runs: Some(2),
            identity: None,
            role_id: None,
            role_system_instructions: None,
        })
        .await?;
    let agent_b = b
        .agents()
        .create_basic(CreateAgent {
            name: format!("B5 Agent B {suffix}"),
            description: Some("tenant B".to_string()),
            max_concurrent_runs: Some(2),
            identity: None,
            role_id: None,
            role_system_instructions: None,
        })
        .await?;

    let project_a = a
        .projects()
        .create_basic(CreateProject {
            name: format!("B5 Project A {suffix}"),
            description: Some("tenant A".to_string()),
            board_preset_id: None,
        })
        .await?;
    let project_b = b
        .projects()
        .create_basic(CreateProject {
            name: format!("B5 Project B {suffix}"),
            description: Some("tenant B".to_string()),
            board_preset_id: None,
        })
        .await?;

    let task_a = a
        .tasks()
        .create(CreateTask {
            name: format!("B5 Task A {suffix}"),
            description: None,
            kind: "shell".to_string(),
            config: json!({"command": "true"}),
            max_duration_seconds: Some(60),
            max_retries: Some(0),
            retry_delay_seconds: Some(1),
            concurrency_policy: Some("allow".to_string()),
            tags: Some(vec!["b5".to_string()]),
            agent_id: Some(agent_a.id.clone()),
            project_id: Some(project_a.id.clone()),
        })
        .await?;
    let task_b = b
        .tasks()
        .create(CreateTask {
            name: format!("B5 Task B {suffix}"),
            description: None,
            kind: "shell".to_string(),
            config: json!({"command": "true"}),
            max_duration_seconds: Some(60),
            max_retries: Some(0),
            retry_delay_seconds: Some(1),
            concurrency_policy: Some("allow".to_string()),
            tags: Some(vec!["b5".to_string()]),
            agent_id: Some(agent_b.id.clone()),
            project_id: Some(project_b.id.clone()),
        })
        .await?;

    let board_a = a
        .project_boards()
        .create(CreateProjectBoard {
            project_id: project_a.id.clone(),
            name: "B5 Board A".to_string(),
            prefix: "B5A".to_string(),
        })
        .await?;
    let board_b = b
        .project_boards()
        .create(CreateProjectBoard {
            project_id: project_b.id.clone(),
            name: "B5 Board B".to_string(),
            prefix: "B5B".to_string(),
        })
        .await?;

    let work_item_a = a
        .work_items()
        .create(CreateWorkItem {
            project_id: project_a.id.clone(),
            board_id: Some(board_a.id.clone()),
            title: format!("B5 Work Item A {suffix}"),
            description: None,
            kind: Some("task".to_string()),
            column_id: None,
            status: Some("todo".to_string()),
            priority: Some(1),
            assignee_agent_id: Some(agent_a.id.clone()),
            created_by_agent_id: Some(agent_a.id.clone()),
            parent_work_item_id: None,
            position: Some(1.0),
            labels: Some(vec!["b5".to_string()]),
            metadata: Some(json!({"tenant": "a"})),
        })
        .await?;
    let work_item_b = b
        .work_items()
        .create(CreateWorkItem {
            project_id: project_b.id.clone(),
            board_id: Some(board_b.id.clone()),
            title: format!("B5 Work Item B {suffix}"),
            description: None,
            kind: Some("task".to_string()),
            column_id: None,
            status: Some("todo".to_string()),
            priority: Some(1),
            assignee_agent_id: Some(agent_b.id.clone()),
            created_by_agent_id: Some(agent_b.id.clone()),
            parent_work_item_id: None,
            position: Some(1.0),
            labels: Some(vec!["b5".to_string()]),
            metadata: Some(json!({"tenant": "b"})),
        })
        .await?;

    let session_a = a
        .chat()
        .create_session(
            agent_a.id.clone(),
            Some(format!("B5 Session A {suffix}")),
            Some("direct".to_string()),
            Some(project_a.id.clone()),
        )
        .await?;
    let session_b = b
        .chat()
        .create_session(
            agent_b.id.clone(),
            Some(format!("B5 Session B {suffix}")),
            Some("direct".to_string()),
            Some(project_b.id.clone()),
        )
        .await?;

    let workflow_a = a
        .project_workflows()
        .create(CreateProjectWorkflow {
            project_id: project_a.id.clone(),
            name: format!("B5 Workflow A {suffix}"),
            description: None,
            trigger_kind: Some("manual".to_string()),
            trigger_config: Some(json!({})),
            graph: Some(WorkflowGraph::default()),
        })
        .await?;
    let workflow_b = b
        .project_workflows()
        .create(CreateProjectWorkflow {
            project_id: project_b.id.clone(),
            name: format!("B5 Workflow B {suffix}"),
            description: None,
            trigger_kind: Some("manual".to_string()),
            trigger_config: Some(json!({})),
            graph: Some(WorkflowGraph::default()),
        })
        .await?;

    let schedule_a = a
        .schedules()
        .create(CreateSchedule {
            task_id: Some(task_a.id.clone()),
            workflow_id: None,
            target_kind: Some("task".to_string()),
            kind: "manual".to_string(),
            config: json!({}),
        })
        .await?;
    let schedule_b = b
        .schedules()
        .create(CreateSchedule {
            task_id: Some(task_b.id.clone()),
            workflow_id: None,
            target_kind: Some("task".to_string()),
            kind: "manual".to_string(),
            config: json!({}),
        })
        .await?;

    let bus_subscription_a = a
        .bus_subscriptions()
        .create(CreateBusSubscription {
            subscriber_agent_id: agent_a.id.clone(),
            source_agent_id: agent_a.id.clone(),
            event_type: "run:completed".to_string(),
            task_id: task_a.id.clone(),
            payload_template: "{}".to_string(),
            max_chain_depth: 3,
        })
        .await?;
    let bus_subscription_b = b
        .bus_subscriptions()
        .create(CreateBusSubscription {
            subscriber_agent_id: agent_b.id.clone(),
            source_agent_id: agent_b.id.clone(),
            event_type: "run:completed".to_string(),
            task_id: task_b.id.clone(),
            payload_template: "{}".to_string(),
            max_chain_depth: 3,
        })
        .await?;

    assert!(a.agents().get(&agent_b.id).await?.is_none());
    assert!(b.agents().get(&agent_a.id).await?.is_none());
    assert!(a.projects().get(&project_b.id).await?.is_none());
    assert!(b.projects().get(&project_a.id).await?.is_none());
    assert!(a.tasks().get(&task_b.id).await?.is_none());
    assert!(b.tasks().get(&task_a.id).await?.is_none());
    assert!(a.project_boards().get(&board_b.id).await?.is_none());
    assert!(b.project_boards().get(&board_a.id).await?.is_none());

    assert!(!a
        .work_items()
        .list(&project_b.id, Some(board_b.id.clone()))
        .await?
        .iter()
        .any(|item| item.id == work_item_b.id));
    assert!(!b
        .work_items()
        .list(&project_a.id, Some(board_a.id.clone()))
        .await?
        .iter()
        .any(|item| item.id == work_item_a.id));
    assert!(a
        .chat()
        .list_sessions(ChatSessionListFilter {
            agent_id: agent_b.id.clone(),
            include_archived: true,
            session_types: Vec::new(),
            project_id: None,
        })
        .await?
        .is_empty());
    assert!(b
        .chat()
        .list_sessions(ChatSessionListFilter {
            agent_id: agent_a.id.clone(),
            include_archived: true,
            session_types: Vec::new(),
            project_id: None,
        })
        .await?
        .is_empty());
    assert!(a
        .project_workflows()
        .list(&project_b.id, 25)
        .await?
        .is_empty());
    assert!(b
        .project_workflows()
        .list(&project_a.id, 25)
        .await?
        .is_empty());
    assert!(a.schedules().list_for_task(&task_b.id).await?.is_empty());
    assert!(b.schedules().list_for_task(&task_a.id).await?.is_empty());
    assert!(a
        .bus_subscriptions()
        .list(Some(agent_b.id.clone()))
        .await?
        .is_empty());
    assert!(b
        .bus_subscriptions()
        .list(Some(agent_a.id.clone()))
        .await?
        .is_empty());

    assert_eq!(
        a.chat().session_meta(&session_a.id).await?.agent_id,
        agent_a.id
    );
    assert_eq!(
        b.chat().session_meta(&session_b.id).await?.agent_id,
        agent_b.id
    );
    assert_eq!(
        a.project_workflows().get(&workflow_a.id).await?.project_id,
        project_a.id
    );
    assert_eq!(
        b.project_workflows().get(&workflow_b.id).await?.project_id,
        project_b.id
    );
    assert_eq!(schedule_a.task_id.as_deref(), Some(task_a.id.as_str()));
    assert_eq!(schedule_b.task_id.as_deref(), Some(task_b.id.as_str()));
    assert_eq!(bus_subscription_a.task_id, task_a.id);
    assert_eq!(bus_subscription_b.task_id, task_b.id);

    cleanup_tenant(&pool, &tenant_a).await;
    cleanup_tenant(&pool, &tenant_b).await;
    Ok(())
}

async fn cleanup_tenant(pool: &PgPool, tenant_id: &str) {
    let tables = [
        "workflow_run_steps",
        "workflow_runs",
        "work_item_events",
        "work_item_comments",
        "work_items",
        "project_board_columns",
        "project_boards",
        "project_workflows",
        "schedules",
        "runs",
        "tasks",
        "bus_messages",
        "bus_subscriptions",
        "active_session_skills",
        "chat_message_reactions",
        "chat_messages",
        "chat_sessions",
        "project_agents",
        "projects",
        "agents",
        "users",
    ];

    for table in tables {
        let query = format!("DELETE FROM {table} WHERE tenant_id = $1");
        let _ = sqlx::query(&query).bind(tenant_id).execute(pool).await;
    }
}
