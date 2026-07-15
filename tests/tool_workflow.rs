mod support;

use appleby::tool::toolmap;
use serde_json::json;

#[tokio::test]
async fn file_tools_complete_write_read_edit_workflow() {
    let temp = support::workspace_tempdir();
    let fixture = std::fs::read_to_string(support::fixture_path("files/multiline.txt")).unwrap();
    let path = temp.path().join("nested").join("workflow.txt");
    let tools = toolmap();

    let write_result = tools
        .get("write_file")
        .unwrap()
        .invoke(&json!({
            "path": path,
            "content": fixture
        }))
        .await
        .unwrap();
    assert!(write_result.starts_with("Wrote 23 bytes to "));

    let initial_content = tools
        .get("read_file")
        .unwrap()
        .invoke(&json!({
            "path": path,
            "start_line": 1,
            "end_line": 4
        }))
        .await
        .unwrap();
    assert_eq!(
        initial_content,
        "1: alpha\n2: beta\n3: gamma\n4: delta\n\nFile total line count: 4\n"
    );

    tools
        .get("edit_file")
        .unwrap()
        .invoke(&json!({
            "file_path": path,
            "old_string": "beta",
            "new_string": "BETA",
            "replace_all": false
        }))
        .await
        .unwrap();

    let edited_line = tools
        .get("read_file")
        .unwrap()
        .invoke(&json!({
            "path": path,
            "start_line": 2,
            "end_line": 2
        }))
        .await
        .unwrap();
    assert_eq!(edited_line, "2: BETA\n\nFile total line count: 4\n");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "alpha\nBETA\ngamma\ndelta\n"
    );
}

#[tokio::test]
async fn public_write_tool_rejects_path_outside_workspace() {
    let outside = tempfile::tempdir().unwrap();
    let path = outside.path().join("should-not-exist.txt");
    let tools = toolmap();

    let error = tools
        .get("write_file")
        .unwrap()
        .invoke(&json!({
            "path": path,
            "content": "blocked"
        }))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("Path escapes workspace"));
    assert!(!path.exists());
}

#[tokio::test]
async fn public_read_tool_handles_unicode_fixture() {
    let path = support::fixture_path("files/unicode.txt");
    let tools = toolmap();

    let result = tools
        .get("read_file")
        .unwrap()
        .invoke(&json!({
            "path": path,
            "start_line": 2,
            "end_line": 3
        }))
        .await
        .unwrap();

    assert_eq!(
        result,
        "2: 第二行🙂\n3: 第三行 café\n\nFile total line count: 3\n"
    );
}
