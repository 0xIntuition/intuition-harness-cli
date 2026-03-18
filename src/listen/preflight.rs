use std::fs;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::Url;

use crate::agents::{command_args_for_invocation, resolve_agent_invocation_for_planning};
use crate::cli::RunAgentArgs;
use crate::config::{AGENT_ROUTE_AGENTS_LISTEN, AppConfig, LinearConfig, PlanningMeta};
use crate::linear::{LinearClient, LinearService};

pub(super) struct ListenPreflightRequest<'a> {
    pub(super) workspace_path: &'a Path,
    pub(super) agent: Option<&'a str>,
    pub(super) model: Option<&'a str>,
    pub(super) reasoning: Option<&'a str>,
}

pub(super) async fn run_listen_preflight<C>(
    service: &LinearService<C>,
    linear_config: &LinearConfig,
    app_config: &AppConfig,
    planning_meta: &PlanningMeta,
    request: ListenPreflightRequest<'_>,
) -> Result<()>
where
    C: LinearClient,
{
    verify_workspace_write_access(request.workspace_path)?;
    verify_network_connectivity(&linear_config.api_url)?;
    verify_linear_api_access(service).await?;

    let invocation = resolve_agent_invocation_for_planning(
        app_config,
        planning_meta,
        &RunAgentArgs {
            root: None,
            route_key: Some(AGENT_ROUTE_AGENTS_LISTEN.to_string()),
            agent: request.agent.map(str::to_string),
            prompt: "listen preflight".to_string(),
            instructions: None,
            model: request.model.map(str::to_string),
            reasoning: request.reasoning.map(str::to_string),
            transport: None,
        },
    )?;
    let command_args = command_args_for_invocation(&invocation, Some(request.workspace_path))?;
    verify_listen_command_capabilities(&invocation.agent, &command_args)?;
    Ok(())
}

pub(super) fn verify_workspace_write_access(workspace_path: &Path) -> Result<()> {
    let probe_path = workspace_path
        .join(".metastack")
        .join(format!(".listen-preflight-{}", std::process::id()));
    if let Some(parent) = probe_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to prepare `{}`", parent.display()))?;
    }
    fs::write(&probe_path, "preflight")
        .with_context(|| format!("workspace `{}` is not writable", workspace_path.display()))?;
    fs::remove_file(&probe_path)
        .with_context(|| format!("failed to remove `{}`", probe_path.display()))?;
    Ok(())
}

pub(super) fn verify_network_connectivity(api_url: &str) -> Result<()> {
    let url = Url::parse(api_url)
        .with_context(|| format!("failed to parse Linear API URL `{api_url}` for preflight"))?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("Linear API URL `{api_url}` does not include a hostname"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("Linear API URL `{api_url}` does not include a known port"))?;
    let mut last_error = None;
    for address in (host, port)
        .to_socket_addrs()
        .with_context(|| format!("failed to resolve `{host}:{port}` during listen preflight"))?
    {
        match TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
            Ok(stream) => {
                drop(stream);
                return Ok(());
            }
            Err(error) => last_error = Some(error),
        }
    }

    let detail = last_error
        .map(|error| error.to_string())
        .unwrap_or_else(|| "no addresses available".to_string());
    bail!("failed to connect to `{host}:{port}` during listen preflight: {detail}");
}

pub(super) async fn verify_linear_api_access<C>(service: &LinearService<C>) -> Result<()>
where
    C: LinearClient,
{
    service
        .viewer()
        .await
        .context("failed to access Linear API during listen preflight")?;
    Ok(())
}

pub(super) fn verify_listen_command_capabilities(
    agent: &str,
    command_args: &[String],
) -> Result<()> {
    match agent {
        "codex" => {
            if command_args
                .iter()
                .any(|arg| arg == "--dangerously-bypass-approvals-and-sandbox")
            {
                return Ok(());
            }
            bail!(
                "listen worker for `codex` requires unrestricted execution; command args were: {}",
                command_args.join(" ")
            );
        }
        "claude" => {
            if command_args.iter().any(|arg| {
                arg == "--dangerously-skip-permissions"
                    || arg == "--permission-mode=bypassPermissions"
            }) || command_args
                .windows(2)
                .any(|pair| pair[0] == "--permission-mode" && pair[1] == "bypassPermissions")
            {
                return Ok(());
            }
            bail!(
                "listen worker for `claude` requires bypassed permissions; command args were: {}",
                command_args.join(" ")
            );
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use anyhow::{Result, anyhow};
    use async_trait::async_trait;
    use tempfile::tempdir;

    use super::{ListenPreflightRequest, run_listen_preflight};
    use crate::config::{AppConfig, LinearConfig, PlanningMeta};
    use crate::linear::{
        AttachmentCreateRequest, AttachmentSummary, IssueComment, IssueCreateRequest,
        IssueLabelCreateRequest, IssueListFilters, IssueSummary, LabelRef, LinearClient,
        LinearService, ProjectSummary, TeamSummary, UserRef,
    };

    #[derive(Debug, Clone)]
    struct StubLinearClient {
        viewer_error: Option<String>,
    }

    #[async_trait]
    impl LinearClient for StubLinearClient {
        async fn list_projects(&self, _: usize) -> Result<Vec<ProjectSummary>> {
            unreachable!("unused in preflight tests")
        }

        async fn list_issues(&self, _: usize) -> Result<Vec<IssueSummary>> {
            unreachable!("unused in preflight tests")
        }

        async fn list_filtered_issues(&self, _: &IssueListFilters) -> Result<Vec<IssueSummary>> {
            unreachable!("unused in preflight tests")
        }

        async fn list_issue_labels(&self, _: Option<&str>) -> Result<Vec<LabelRef>> {
            unreachable!("unused in preflight tests")
        }

        async fn get_issue(&self, _: &str) -> Result<IssueSummary> {
            unreachable!("unused in preflight tests")
        }

        async fn list_teams(&self) -> Result<Vec<TeamSummary>> {
            unreachable!("unused in preflight tests")
        }

        async fn viewer(&self) -> Result<UserRef> {
            if let Some(error) = &self.viewer_error {
                Err(anyhow!(error.clone()))
            } else {
                Ok(UserRef {
                    id: "viewer-1".to_string(),
                    name: "Viewer".to_string(),
                    email: Some("viewer@example.com".to_string()),
                })
            }
        }

        async fn create_issue(&self, _: IssueCreateRequest) -> Result<IssueSummary> {
            unreachable!("unused in preflight tests")
        }

        async fn create_issue_label(&self, _: IssueLabelCreateRequest) -> Result<LabelRef> {
            unreachable!("unused in preflight tests")
        }

        async fn update_issue(
            &self,
            _: &str,
            _: crate::linear::IssueUpdateRequest,
        ) -> Result<IssueSummary> {
            unreachable!("unused in preflight tests")
        }

        async fn create_comment(&self, _: &str, _: String) -> Result<IssueComment> {
            unreachable!("unused in preflight tests")
        }

        async fn update_comment(&self, _: &str, _: String) -> Result<IssueComment> {
            unreachable!("unused in preflight tests")
        }

        async fn upload_file(&self, _: &str, _: &str, _: Vec<u8>) -> Result<String> {
            unreachable!("unused in preflight tests")
        }

        async fn create_attachment(&self, _: AttachmentCreateRequest) -> Result<AttachmentSummary> {
            unreachable!("unused in preflight tests")
        }

        async fn delete_attachment(&self, _: &str) -> Result<()> {
            unreachable!("unused in preflight tests")
        }

        async fn download_file(&self, _: &str) -> Result<Vec<u8>> {
            unreachable!("unused in preflight tests")
        }
    }

    #[tokio::test]
    async fn listen_preflight_accepts_reachable_linear_api_and_viewer_access() -> Result<()> {
        let temp = tempdir()?;
        let workspace = temp.path();
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let linear_config = LinearConfig {
            api_key: "token".to_string(),
            api_url: format!("http://{}/graphql", listener.local_addr()?),
            default_team: None,
        };
        let service = LinearService::new(
            StubLinearClient { viewer_error: None },
            linear_config.default_team.clone(),
        );

        run_listen_preflight(
            &service,
            &linear_config,
            &AppConfig::default(),
            &PlanningMeta::default(),
            ListenPreflightRequest {
                workspace_path: workspace,
                agent: Some("claude"),
                model: Some("sonnet"),
                reasoning: Some("high"),
            },
        )
        .await?;

        Ok(())
    }

    #[tokio::test]
    async fn listen_preflight_rejects_missing_linear_api_access() -> Result<()> {
        let temp = tempdir()?;
        let workspace = temp.path();
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let linear_config = LinearConfig {
            api_key: "token".to_string(),
            api_url: format!("http://{}/graphql", listener.local_addr()?),
            default_team: None,
        };
        let service = LinearService::new(
            StubLinearClient {
                viewer_error: Some("unauthorized".to_string()),
            },
            linear_config.default_team.clone(),
        );

        let error = run_listen_preflight(
            &service,
            &linear_config,
            &AppConfig::default(),
            &PlanningMeta::default(),
            ListenPreflightRequest {
                workspace_path: workspace,
                agent: Some("claude"),
                model: Some("sonnet"),
                reasoning: Some("high"),
            },
        )
        .await
        .expect_err("viewer access should be required");

        assert!(
            error
                .to_string()
                .contains("failed to access Linear API during listen preflight")
        );
        Ok(())
    }
}
