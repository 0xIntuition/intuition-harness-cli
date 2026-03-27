#![allow(dead_code, unused_imports)]

include!("support/common.rs");

use metastack_cli::branding;

fn write_onboarded_config(config_path: &Path, body: &str) -> Result<(), Box<dyn Error>> {
    let body = body.trim_start();
    let content = if body.is_empty() {
        "[onboarding]\ncompleted = true\n".to_string()
    } else {
        format!("[onboarding]\ncompleted = true\n\n{body}")
    };
    fs::write(config_path, content)?;
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), Box<dyn Error>> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(unix)]
fn clone_workspace(remote: &Path, workspace: &Path) -> Result<(), Box<dyn Error>> {
    let status = ProcessCommand::new("git")
        .args([
            "clone",
            remote.to_string_lossy().as_ref(),
            workspace.to_string_lossy().as_ref(),
        ])
        .status()?;
    assert!(status.success());
    Ok(())
}

#[cfg(unix)]
#[test]
fn agents_build_resolves_ticket_workspace_and_uses_command_route_provider()
-> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_dir = temp.path().join("stub-output");
    let bin_dir = temp.path().join("bin");
    let workspace_path = temp.path().join("repo-workspace").join("ENG-10507");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&stub_dir)?;
    fs::create_dir_all(&bin_dir)?;

    write_minimal_planning_context(
        &repo_root,
        r#"{
  "agent": {
    "provider": "repo-stub"
  }
}
"#,
    )?;
    fs::write(repo_root.join("README.md"), "# Build Demo\n")?;
    let remote = init_repo_with_origin(&repo_root)?;
    clone_workspace(&remote, &workspace_path)?;

    let global_stub = bin_dir.join("global-stub");
    let build_stub = bin_dir.join("build-stub");
    fs::write(
        &global_stub,
        "#!/bin/sh\nprintf 'global-stub should not run\\n'\nexit 99\n",
    )?;
    fs::write(
        &build_stub,
        r#"#!/bin/sh
printf '%s' "$1" > "$TEST_OUTPUT_DIR/prompt.txt"
printf '%s' "$METASTACK_AGENT_ROUTE_KEY" > "$TEST_OUTPUT_DIR/route-key.txt"
printf '%s' "$METASTACK_AGENT_PROVIDER_SOURCE" > "$TEST_OUTPUT_DIR/provider-source.txt"
printf 'stub stdout: %s\n' "$1"
printf 'stub stderr\n' >&2
"#,
    )?;
    make_executable(&global_stub)?;
    make_executable(&build_stub)?;

    let config_body = format!(
        r#"[agents]
default_agent = "global-stub"

[agents.routing.commands."agents.build"]
provider = "build-stub"

[agents.commands.global-stub]
command = "{global}"
args = ["{{{{payload}}}}"]
transport = "arg"

[agents.commands.build-stub]
command = "{build}"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
        global = global_stub.display(),
        build = build_stub.display(),
    );
    write_onboarded_config(&config_path, &config_body)?;

    let output = cli()
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env(
            "PATH",
            format!("{}:{}", bin_dir.display(), std::env::var("PATH")?),
        )
        .args([
            "agents",
            "build",
            "ENG-10507",
            "fix the auth bug",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--no-interactive",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    assert!(stdout.contains("workspace="));
    assert!(stdout.contains("provider=build-stub"));
    assert!(stdout.contains("completed successfully"));
    assert!(stderr.contains("stub stderr"));
    assert_eq!(
        fs::read_to_string(stub_dir.join("prompt.txt"))?,
        "fix the auth bug"
    );
    assert_eq!(
        fs::read_to_string(stub_dir.join("route-key.txt"))?,
        "agents.build"
    );
    assert_eq!(
        fs::read_to_string(stub_dir.join("provider-source.txt"))?,
        "command_route:agents.build"
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn agents_build_dir_mode_treats_workspace_positional_as_prompt_and_cli_override_wins()
-> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let stub_dir = temp.path().join("stub-output");
    let bin_dir = temp.path().join("bin");
    let explicit_workspace = temp.path().join("custom-workspace");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&stub_dir)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&explicit_workspace)?;

    write_minimal_planning_context(&repo_root, "{}")?;
    fs::write(repo_root.join("README.md"), "# Build Demo\n")?;
    init_repo_with_origin(&repo_root)?;
    let status = ProcessCommand::new("git")
        .args([
            "-C",
            explicit_workspace.to_string_lossy().as_ref(),
            "init",
            "-b",
            "main",
        ])
        .status()?;
    assert!(status.success());

    let route_stub = bin_dir.join("route-stub");
    let override_stub = bin_dir.join("override-stub");
    fs::write(
        &route_stub,
        "#!/bin/sh\nprintf 'route-stub should not run\\n'\nexit 99\n",
    )?;
    fs::write(
        &override_stub,
        r#"#!/bin/sh
printf '%s' "$1" > "$TEST_OUTPUT_DIR/prompt.txt"
printf '%s' "$METASTACK_AGENT_PROVIDER_SOURCE" > "$TEST_OUTPUT_DIR/provider-source.txt"
printf 'override stdout: %s\n' "$1"
"#,
    )?;
    make_executable(&route_stub)?;
    make_executable(&override_stub)?;

    let config_body = format!(
        r#"[agents]
default_agent = "route-stub"

[agents.routing.commands."agents.build"]
provider = "route-stub"

[agents.commands.route-stub]
command = "{route}"
args = ["{{{{payload}}}}"]
transport = "arg"

[agents.commands.override-stub]
command = "{override_cmd}"
args = ["{{{{payload}}}}"]
transport = "arg"
"#,
        route = route_stub.display(),
        override_cmd = override_stub.display(),
    );
    write_onboarded_config(&config_path, &config_body)?;

    let output = cli()
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &stub_dir)
        .env(
            "PATH",
            format!("{}:{}", bin_dir.display(), std::env::var("PATH")?),
        )
        .args([
            "agents",
            "build",
            "--dir",
            explicit_workspace.to_string_lossy().as_ref(),
            "tighten the failing CLI test",
            "--agent",
            "override-stub",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--no-interactive",
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8(output.stdout)?;
    assert!(stdout.contains("provider=override-stub"));
    assert!(stdout.contains("completed successfully"));
    assert_eq!(
        fs::read_to_string(stub_dir.join("prompt.txt"))?,
        "tighten the failing CLI test"
    );
    assert_eq!(
        fs::read_to_string(stub_dir.join("provider-source.txt"))?,
        "explicit_override"
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn agents_build_rejects_non_git_workspace() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let config_path = temp.path().join("metastack.toml");
    let workspace_path = temp.path().join("repo-workspace").join("ENG-10507");
    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&workspace_path)?;

    write_minimal_planning_context(&repo_root, "{}")?;
    write_onboarded_config(
        &config_path,
        r#"[agents]
default_agent = "codex"
"#,
    )?;

    cli()
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "agents",
            "build",
            "ENG-10507",
            "fix the auth bug",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--no-interactive",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("is not a git repository"));

    Ok(())
}
