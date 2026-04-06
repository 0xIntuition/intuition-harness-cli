#![allow(dead_code, unused_imports)]

include!("support/common.rs");

use metastack_cli::branding;

#[cfg(unix)]
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
fn init_git_repo(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(path)?;
    let status = ProcessCommand::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .status()?;
    assert!(status.success());
    Ok(())
}

#[cfg(unix)]
fn write_build_stub(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::write(
        path,
        r#"#!/bin/sh
printf '%s' "$PWD" > "$TEST_OUTPUT_DIR/cwd.txt"
printf '%s' "$METASTACK_AGENT_ROUTE_KEY" > "$TEST_OUTPUT_DIR/route-key.txt"
printf '%s' "$METASTACK_AGENT_MODEL" > "$TEST_OUTPUT_DIR/model.txt"
printf '%s' "$METASTACK_AGENT_REASONING" > "$TEST_OUTPUT_DIR/reasoning.txt"
cat > "$TEST_OUTPUT_DIR/prompt.txt"
echo "stub stdout"
echo "stub stderr" >&2
"#,
    )?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn agents_build_runs_in_explicit_workspace_and_uses_build_route_defaults()
-> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let workspace_dir = temp.path().join("workspace");
    let output_dir = temp.path().join("output");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("build-agent-stub");

    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
    init_git_repo(&workspace_dir)?;
    write_build_stub(&stub_path)?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[agents.routing.commands."agents.build"]
provider = "build-stub"
model = "test-model"
reasoning = "high"

[agents.commands.build-stub]
command = "{}"
transport = "stdin"
"#,
            stub_path.display()
        )
        .as_str(),
    )?;

    cli()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "agents",
            "build",
            "--dir",
            workspace_dir.to_string_lossy().as_ref(),
            "fix the auth bug",
            "--no-interactive",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("stub stdout"))
        .stderr(predicate::str::contains("stub stderr"))
        .stderr(predicate::str::contains("Run #1"))
        .stderr(predicate::str::contains(
            "Completion summary: status=success",
        ))
        .stderr(predicate::str::contains("tokens unavailable"));

    assert_eq!(
        fs::read_to_string(output_dir.join("cwd.txt"))?,
        workspace_dir.canonicalize()?.display().to_string()
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("route-key.txt"))?,
        "agents.build"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("model.txt"))?,
        "test-model"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("reasoning.txt"))?,
        "high"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("prompt.txt"))?,
        "Prompt:\nfix the auth bug\n\nPreferred model:\ntest-model\n\nPreferred reasoning effort:\nhigh"
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn agents_build_resolves_ticket_workspace_from_sibling_root() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let workspace_dir = temp.path().join("repo-workspace").join("MET-45");
    let output_dir = temp.path().join("output");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("build-agent-stub");

    init_git_repo(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
    init_git_repo(&workspace_dir)?;
    write_build_stub(&stub_path)?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[agents]
default_agent = "build-stub"

[agents.commands.build-stub]
command = "{}"
transport = "stdin"
"#,
            stub_path.display()
        )
        .as_str(),
    )?;

    cli()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "agents",
            "build",
            "MET-45",
            "run qa",
            "--root",
            repo_root.to_string_lossy().as_ref(),
            "--no-interactive",
        ])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(output_dir.join("cwd.txt"))?,
        workspace_dir.canonicalize()?.display().to_string()
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn agents_build_preserves_explicit_nested_workspace_directory() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let workspace_dir = temp.path().join("workspace");
    let nested_dir = workspace_dir.join("qa").join("focus");
    let output_dir = temp.path().join("output");
    let config_path = temp.path().join("metastack.toml");
    let stub_path = temp.path().join("build-agent-stub");

    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&output_dir)?;
    init_git_repo(&workspace_dir)?;
    fs::create_dir_all(&nested_dir)?;
    write_build_stub(&stub_path)?;
    write_onboarded_config(
        &config_path,
        format!(
            r#"[agents]
default_agent = "build-stub"

[agents.commands.build-stub]
command = "{}"
transport = "stdin"
"#,
            stub_path.display()
        )
        .as_str(),
    )?;

    cli()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .env("TEST_OUTPUT_DIR", &output_dir)
        .args([
            "agents",
            "build",
            "--dir",
            nested_dir.to_string_lossy().as_ref(),
            "run qa",
            "--no-interactive",
        ])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(output_dir.join("cwd.txt"))?,
        nested_dir.canonicalize()?.display().to_string()
    );

    Ok(())
}

#[cfg(unix)]
#[test]
fn agents_build_rejects_non_git_directories() -> Result<(), Box<dyn Error>> {
    let temp = tempdir()?;
    let repo_root = temp.path().join("repo");
    let workspace_dir = temp.path().join("workspace");
    let config_path = temp.path().join("metastack.toml");

    fs::create_dir_all(&repo_root)?;
    fs::create_dir_all(&workspace_dir)?;
    write_onboarded_config(&config_path, "")?;

    cli()
        .current_dir(&repo_root)
        .env("METASTACK_CONFIG", &config_path)
        .args([
            "agents",
            "build",
            "--dir",
            workspace_dir.to_string_lossy().as_ref(),
            "run qa",
            "--no-interactive",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "workspace directory is not a git repository",
        ));

    Ok(())
}
