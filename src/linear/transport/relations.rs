use anyhow::{Result, bail};
use serde_json::json;

use crate::linear::IssueRelationCreateRequest;

use super::{ReqwestLinearClient, model::IssueRelationCreatePayload};

impl ReqwestLinearClient {
    pub(super) async fn create_issue_relation_resource(
        &self,
        request: IssueRelationCreateRequest,
    ) -> Result<()> {
        let query = r#"
mutation CreateIssueRelation($input: IssueRelationCreateInput!) {
  issueRelationCreate(input: $input) {
    success
  }
}
"#;
        let data: IssueRelationCreatePayload = self
            .graphql()
            .query(
                query,
                json!({
                    "input": {
                        "issueId": request.issue_id,
                        "relatedIssueId": request.related_issue_id,
                        "type": request.relation_type,
                    }
                }),
            )
            .await?;

        if !data.issue_relation_create.success {
            bail!("Linear did not confirm issue relation creation");
        }

        Ok(())
    }
}
