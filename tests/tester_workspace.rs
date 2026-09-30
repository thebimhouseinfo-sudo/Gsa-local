use gsa_local::{
    harness::AgentId,
    tester_workspace::{TesterArtifactRef, TesterWorkspaceRuntime},
    tools::ProjectToolRuntime,
};
use serde_json::json;
use tempfile::tempdir;

#[test]
fn tester_workspace_is_attempt_scoped_and_returns_stable_artifact_refs() {
    let dir = tempdir().unwrap();
    let workspace = TesterWorkspaceRuntime::new(dir.path(), 7, "CP-SESSION", "ATT-1").unwrap();

    for subdir in ["plan", "tests", "fixtures", "artifacts", "reports"] {
        assert!(workspace.root().join(subdir).is_dir());
    }

    let tester_tools = workspace
        .tool_definitions(AgentId::Tester)
        .into_iter()
        .map(|tool| tool.function.name)
        .collect::<Vec<_>>();
    assert!(tester_tools
        .iter()
        .any(|name| name == "tester_workspace_write"));
    assert!(workspace.tool_definitions(AgentId::Coder).is_empty());

    let created = workspace
        .execute(
            AgentId::Tester,
            "tester_workspace_write",
            &json!({
                "path":"tests/session_probe.txt",
                "content":"sample-1",
                "create_only":true
            }),
        )
        .unwrap();
    let created_ref: TesterArtifactRef =
        serde_json::from_value(created["artifact"].clone()).unwrap();
    assert_eq!(created_ref.graph_version, 7);
    assert_eq!(created_ref.checkpoint_id, "CP-SESSION");
    assert_eq!(created_ref.attempt_id, "ATT-1");
    assert_eq!(created_ref.path, "tests/session_probe.txt");

    let read = workspace
        .execute(
            AgentId::Tester,
            "tester_workspace_read",
            &json!({"path":"tests/session_probe.txt"}),
        )
        .unwrap();
    assert_eq!(read["content"], "sample-1");
    assert_eq!(read["artifact"]["sha256"], created["artifact"]["sha256"]);

    let updated = workspace
        .execute(
            AgentId::Tester,
            "tester_workspace_write",
            &json!({
                "path":"tests/session_probe.txt",
                "content":"sample-2",
                "expected_sha256":created_ref.sha256
            }),
        )
        .unwrap();
    assert_ne!(
        updated["artifact"]["sha256"].as_str().unwrap(),
        created["artifact"]["sha256"].as_str().unwrap()
    );

    let direct = workspace.artifact_ref("tests/session_probe.txt").unwrap();
    assert_eq!(
        direct.sha256,
        updated["artifact"]["sha256"].as_str().unwrap()
    );

    let listed = workspace
        .execute(
            AgentId::Tester,
            "tester_workspace_list",
            &json!({"path":"tests"}),
        )
        .unwrap();
    assert!(listed["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "tests/session_probe.txt"));
}

#[test]
fn tester_workspace_tools_are_tester_only_and_product_write_stays_denied() {
    let dir = tempdir().unwrap();
    let workspace = TesterWorkspaceRuntime::new(dir.path(), 1, "CP-1", "ATT-1").unwrap();

    assert!(workspace
        .execute(
            AgentId::Coder,
            "tester_workspace_write",
            &json!({
                "path":"tests/no.txt",
                "content":"blocked",
                "create_only":true
            }),
        )
        .is_err());

    let mut product = ProjectToolRuntime::new(dir.path()).unwrap();
    assert!(product
        .execute(
            AgentId::Tester,
            "project_write",
            &json!({
                "path":"product.txt",
                "content":"blocked",
                "create_only":true
            }),
        )
        .is_err());
    assert!(!dir.path().join("product.txt").exists());
}

#[test]
fn tester_workspace_rejects_unsafe_identity_and_path_escape() {
    let dir = tempdir().unwrap();

    assert!(TesterWorkspaceRuntime::new(dir.path(), 0, "CP-1", "ATT-1").is_err());
    assert!(TesterWorkspaceRuntime::new(dir.path(), 1, "../CP", "ATT-1").is_err());
    assert!(TesterWorkspaceRuntime::new(dir.path(), 1, "CP-1", "ATT/1").is_err());

    let workspace = TesterWorkspaceRuntime::new(dir.path(), 1, "CP-1", "ATT-1").unwrap();
    assert!(workspace
        .execute(
            AgentId::Tester,
            "tester_workspace_write",
            &json!({
                "path":"../outside.txt",
                "content":"blocked",
                "create_only":true
            }),
        )
        .is_err());
    assert!(workspace
        .execute(
            AgentId::Tester,
            "tester_workspace_write",
            &json!({
                "path":"/tmp/outside.txt",
                "content":"blocked",
                "create_only":true
            }),
        )
        .is_err());
}

#[cfg(unix)]
#[test]
fn tester_workspace_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let project = tempdir().unwrap();
    let outside = tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();

    let workspace = TesterWorkspaceRuntime::new(project.path(), 1, "CP-1", "ATT-1").unwrap();
    symlink(outside.path(), workspace.root().join("tests/link")).unwrap();

    assert!(workspace
        .execute(
            AgentId::Tester,
            "tester_workspace_read",
            &json!({"path":"tests/link/secret.txt"}),
        )
        .is_err());
    assert!(workspace
        .execute(
            AgentId::Tester,
            "tester_workspace_write",
            &json!({
                "path":"tests/link/new.txt",
                "content":"blocked",
                "create_only":true
            }),
        )
        .is_err());
    assert!(!outside.path().join("new.txt").exists());
}

#[cfg(unix)]
#[test]
fn tester_workspace_rejects_hardlink_aliases() {
    let project = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let outside_file = outside.path().join("outside.txt");
    std::fs::write(&outside_file, "outside").unwrap();

    let workspace = TesterWorkspaceRuntime::new(project.path(), 1, "CP-1", "ATT-1").unwrap();
    let linked = workspace.root().join("artifacts/linked.txt");
    std::fs::hard_link(&outside_file, &linked).unwrap();

    assert!(workspace.artifact_ref("artifacts/linked.txt").is_err());
    assert!(workspace
        .execute(
            AgentId::Tester,
            "tester_workspace_read",
            &json!({"path":"artifacts/linked.txt"}),
        )
        .is_err());
    assert!(workspace
        .execute(
            AgentId::Tester,
            "tester_workspace_write",
            &json!({
                "path":"artifacts/linked.txt",
                "content":"tampered",
                "expected_sha256":"not-used"
            }),
        )
        .is_err());

    assert_eq!(std::fs::read_to_string(outside_file).unwrap(), "outside");
}
