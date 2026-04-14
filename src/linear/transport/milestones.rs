use anyhow::Result;
use serde_json::json;

use crate::linear::ProjectMilestoneSummary;

use super::{
    ReqwestLinearClient,
    model::{ProjectMilestoneNode, ProjectMilestonesPayload},
    pagination::CursorPager,
};

const PROJECT_MILESTONES_PAGE_SIZE: usize = 100;

const PROJECT_MILESTONES_QUERY: &str = r#"
query ProjectMilestones($projectId: ID!, $first: Int!, $after: String) {
  projectMilestones(
    first: $first
    after: $after
    filter: { project: { id: { eq: $projectId } } }
  ) {
    nodes {
      id
      name
      targetDate
      project {
        id
        name
      }
    }
    pageInfo {
      hasNextPage
      endCursor
    }
  }
}
"#;

impl ReqwestLinearClient {
    pub(super) async fn list_project_milestones_resource(
        &self,
        project_id: &str,
        limit: usize,
    ) -> Result<Vec<ProjectMilestoneSummary>> {
        let mut milestones = Vec::new();
        let mut pager = CursorPager::new(Some(limit.max(1)), PROJECT_MILESTONES_PAGE_SIZE);

        while let Some(first) = pager.next_page_size() {
            let data: ProjectMilestonesPayload = self
                .graphql()
                .query(
                    PROJECT_MILESTONES_QUERY,
                    json!({
                        "projectId": project_id,
                        "first": first,
                        "after": pager.after(),
                    }),
                )
                .await?;
            let page = data.project_milestones;
            pager.advance(&page);
            milestones.extend(page.nodes.into_iter().map(ProjectMilestoneNode::into));
        }

        Ok(milestones)
    }
}
