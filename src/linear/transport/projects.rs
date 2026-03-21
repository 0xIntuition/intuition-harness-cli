use anyhow::{Result, anyhow, bail};
use serde_json::json;

use crate::linear::{ProjectSummary, ProjectUpdateRequest};

use super::{
    ReqwestLinearClient,
    model::{ProjectByIdPayload, ProjectUpdatePayload, ProjectsPayload},
};

const PROJECTS_QUERY: &str = r#"
query Projects($first: Int!) {
  projects(first: $first) {
    nodes {
      id
      name
      description
      url
      progress
      teams(first: 10) {
        nodes {
          id
          key
          name
        }
      }
    }
  }
}
"#;

const PROJECT_QUERY: &str = r#"
query Project($id: String!) {
  project(id: $id) {
    id
    name
    description
    url
    progress
    teams(first: 10) {
      nodes {
        id
        key
        name
      }
    }
  }
}
"#;

const UPDATE_PROJECT_MUTATION: &str = r#"
mutation UpdateProject($id: String!, $input: ProjectUpdateInput!) {
  projectUpdate(id: $id, input: $input) {
    success
    project {
      id
      name
      description
      url
      progress
      teams(first: 10) {
        nodes {
          id
          key
          name
        }
      }
    }
  }
}
"#;

impl ReqwestLinearClient {
    pub(super) async fn list_projects_resource(&self, limit: usize) -> Result<Vec<ProjectSummary>> {
        let data: ProjectsPayload = self
            .graphql()
            .query(PROJECTS_QUERY, json!({ "first": limit.max(1) }))
            .await?;

        Ok(data
            .projects
            .nodes
            .into_iter()
            .map(ProjectSummary::from)
            .collect())
    }

    pub(super) async fn get_project_resource(&self, project_id: &str) -> Result<ProjectSummary> {
        let data: ProjectByIdPayload = self
            .graphql()
            .query(PROJECT_QUERY, json!({ "id": project_id }))
            .await?;

        data.project
            .map(ProjectSummary::from)
            .ok_or_else(|| anyhow!("project `{project_id}` was not found in Linear"))
    }

    pub(super) async fn update_project_resource(
        &self,
        project_id: &str,
        request: ProjectUpdateRequest,
    ) -> Result<ProjectSummary> {
        let data: ProjectUpdatePayload = self
            .graphql()
            .query(
                UPDATE_PROJECT_MUTATION,
                json!({
                    "id": project_id,
                    "input": {
                        "description": request.description,
                    }
                }),
            )
            .await?;

        if !data.project_update.success {
            bail!("Linear reported project update failure for `{project_id}`");
        }

        data.project_update
            .project
            .map(ProjectSummary::from)
            .ok_or_else(|| anyhow!("Linear returned no updated project for `{project_id}`"))
    }
}
