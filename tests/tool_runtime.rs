use gsa_local::{harness::AgentId, tools::ProjectToolRuntime};
use serde_json::json;
use tempfile::tempdir;

#[test]
fn boundary_rejects_absolute_and_parent_traversal() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
    let mut runtime = ProjectToolRuntime::new(dir.path()).unwrap();

    assert!(runtime
        .execute(
            AgentId::Coder,
            "project_read",
            &json!({"path":"../outside.txt"})
        )
        .is_err());
    assert!(runtime
        .execute(
            AgentId::Coder,
            "project_read",
            &json!({"path":"/etc/passwd"})
        )
        .is_err());
}

#[test]
fn project_read_refuses_partial_large_file_content() {
    let dir = tempdir().unwrap();
    let large = "x".repeat(64 * 1024 + 1);
    std::fs::write(dir.path().join("large.txt"), large).unwrap();
    let mut runtime = ProjectToolRuntime::new(dir.path()).unwrap();

    assert!(runtime
        .execute(AgentId::Coder, "project_read", &json!({"path":"large.txt"}))
        .is_err());
    assert!(runtime.journal().is_empty());
}

#[cfg(unix)]
#[test]
fn boundary_rejects_hardlink_write_that_could_mutate_outside_project() {
    let root = tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let outside = root.path().join("outside.txt");
    std::fs::write(&outside, "outside").unwrap();
    std::fs::hard_link(&outside, project.join("linked.txt")).unwrap();

    let mut runtime = ProjectToolRuntime::new(&project).unwrap();
    let read = runtime
        .execute(
            AgentId::Coder,
            "project_read",
            &json!({"path":"linked.txt"}),
        )
        .unwrap();

    assert!(runtime
        .execute(
            AgentId::Coder,
            "project_write",
            &json!({
                "path":"linked.txt",
                "content":"changed",
                "expected_sha256":read["sha256"]
            })
        )
        .is_err());
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "outside");
    assert!(runtime.journal().is_empty());
}

#[cfg(unix)]
#[test]
fn boundary_rejects_symlink_escape_for_read_and_write() {
    use std::os::unix::fs::symlink;

    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    symlink(outside.path(), root.path().join("link")).unwrap();

    let mut runtime = ProjectToolRuntime::new(root.path()).unwrap();
    assert!(runtime
        .execute(
            AgentId::Coder,
            "project_read",
            &json!({"path":"link/secret.txt"})
        )
        .is_err());
    assert!(runtime
        .execute(
            AgentId::Coder,
            "project_write",
            &json!({
                "path":"link/new.txt",
                "content":"no",
                "create_only":true
            })
        )
        .is_err());
}

#[test]
fn existing_write_requires_fresh_read_hash() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "one").unwrap();
    let mut runtime = ProjectToolRuntime::new(dir.path()).unwrap();

    let read = runtime
        .execute(AgentId::Coder, "project_read", &json!({"path":"a.txt"}))
        .unwrap();
    let sha = read["sha256"].as_str().unwrap().to_owned();

    assert!(runtime
        .execute(
            AgentId::Coder,
            "project_write",
            &json!({
                "path":"a.txt",
                "content":"two",
                "expected_sha256":"deadbeef"
            })
        )
        .is_err());
    assert!(runtime.journal().is_empty());

    let written = runtime
        .execute(
            AgentId::Coder,
            "project_write",
            &json!({
                "path":"a.txt",
                "content":"two",
                "expected_sha256":sha
            }),
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "two"
    );
    assert_eq!(runtime.journal().len(), 1);
    assert_eq!(
        written["before_sha256"].as_str().unwrap(),
        read["sha256"].as_str().unwrap()
    );
}

#[test]
fn create_only_never_overwrites_existing_file() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "one").unwrap();
    let mut runtime = ProjectToolRuntime::new(dir.path()).unwrap();

    assert!(runtime
        .execute(
            AgentId::Coder,
            "project_write",
            &json!({
                "path":"a.txt",
                "content":"two",
                "create_only":true
            })
        )
        .is_err());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one"
    );

    runtime
        .execute(
            AgentId::Coder,
            "project_write",
            &json!({
                "path":"b.txt",
                "content":"new",
                "create_only":true
            }),
        )
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "new"
    );
}

#[test]
fn reviewer_and_cr_are_read_only_but_coder_can_write() {
    let dir = tempdir().unwrap();
    let mut runtime = ProjectToolRuntime::new(dir.path()).unwrap();

    for agent in [AgentId::Reviewer, AgentId::LocalCr] {
        let names = runtime
            .tool_definitions(agent)
            .into_iter()
            .map(|tool| tool.function.name)
            .collect::<Vec<_>>();
        assert!(!names.iter().any(|name| name == "project_write"));
        assert!(runtime
            .execute(
                agent,
                "project_write",
                &json!({
                    "path":"blocked.txt",
                    "content":"no",
                    "create_only":true
                })
            )
            .is_err());
    }

    let coder_names = runtime
        .tool_definitions(AgentId::Coder)
        .into_iter()
        .map(|tool| tool.function.name)
        .collect::<Vec<_>>();
    assert!(coder_names.iter().any(|name| name == "project_write"));
}

#[test]
fn mutation_change_set_id_is_deterministic_and_order_sensitive() {
    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();
    for dir in [&dir_a, &dir_b] {
        std::fs::write(dir.path().join("a.txt"), "one").unwrap();
    }

    let mut a = ProjectToolRuntime::new(dir_a.path()).unwrap();
    let mut b = ProjectToolRuntime::new(dir_b.path()).unwrap();

    for runtime in [&mut a, &mut b] {
        let read = runtime
            .execute(AgentId::Coder, "project_read", &json!({"path":"a.txt"}))
            .unwrap();
        runtime
            .execute(
                AgentId::Coder,
                "project_write",
                &json!({
                    "path":"a.txt",
                    "content":"two",
                    "expected_sha256":read["sha256"]
                }),
            )
            .unwrap();
        runtime
            .execute(
                AgentId::Coder,
                "project_write",
                &json!({
                    "path":"b.txt",
                    "content":"three",
                    "create_only":true
                }),
            )
            .unwrap();
    }

    assert_eq!(a.change_set_id().unwrap(), b.change_set_id().unwrap());

    let dir_c = tempdir().unwrap();
    std::fs::write(dir_c.path().join("a.txt"), "one").unwrap();
    let mut c = ProjectToolRuntime::new(dir_c.path()).unwrap();
    c.execute(
        AgentId::Coder,
        "project_write",
        &json!({"path":"b.txt","content":"three","create_only":true}),
    )
    .unwrap();
    let read = c
        .execute(AgentId::Coder, "project_read", &json!({"path":"a.txt"}))
        .unwrap();
    c.execute(
        AgentId::Coder,
        "project_write",
        &json!({
            "path":"a.txt",
            "content":"two",
            "expected_sha256":read["sha256"]
        }),
    )
    .unwrap();

    assert_ne!(a.change_set_id().unwrap(), c.change_set_id().unwrap());
}

#[test]
fn list_and_search_are_bounded_to_project_and_skip_generated_dirs() {
    let dir = tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::create_dir(dir.path().join("target")).unwrap();
    std::fs::write(dir.path().join("src/a.txt"), "needle").unwrap();
    std::fs::write(dir.path().join("target/ignored.txt"), "needle").unwrap();

    let mut runtime = ProjectToolRuntime::new(dir.path()).unwrap();
    let list = runtime
        .execute(AgentId::Reviewer, "project_list", &json!({}))
        .unwrap();
    let files = list["files"].as_array().unwrap();
    assert!(files.iter().any(|item| item == "src/a.txt"));
    assert!(!files.iter().any(|item| item == "target/ignored.txt"));

    let search = runtime
        .execute(
            AgentId::Reviewer,
            "project_search",
            &json!({"query":"needle"}),
        )
        .unwrap();
    let matches = search["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["path"], "src/a.txt");
}

#[test]
fn mutation_journal_rejects_external_source_change() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "one").unwrap();
    let mut runtime = ProjectToolRuntime::new(dir.path()).unwrap();

    let read = runtime
        .execute(AgentId::Coder, "project_read", &json!({"path":"a.txt"}))
        .unwrap();
    runtime
        .execute(
            AgentId::Coder,
            "project_write",
            &json!({
                "path":"a.txt",
                "content":"two",
                "expected_sha256":read["sha256"]
            }),
        )
        .unwrap();

    runtime.verify_journal_current().unwrap();
    std::fs::write(dir.path().join("a.txt"), "external edit").unwrap();
    assert!(runtime.verify_journal_current().is_err());
}
