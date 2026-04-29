use crate::db::repos::sqlite::SqliteRepos;
use crate::db::repos::ProjectRepo;
use crate::db::DbPool;

pub async fn assert_agent_in_project(
    db: &DbPool,
    project_id: &str,
    agent_id: &str,
) -> Result<(), String> {
    let repos = SqliteRepos::new(db.clone());
    if repos.agent_in_project(project_id, agent_id).await? {
        return Ok(());
    }

    Err(format!(
        "agent '{}' is not a member of project '{}'",
        agent_id, project_id
    ))
}
