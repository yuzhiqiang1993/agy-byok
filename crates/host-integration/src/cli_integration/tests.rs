use super::patch;
use super::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn cli_integration_state_round_trips_with_snake_case_names() {
    for (state, serialized) in [
        (CliIntegrationState::Disabled, "disabled"),
        (CliIntegrationState::Managed, "managed"),
        (CliIntegrationState::Mismatch, "mismatch"),
        (CliIntegrationState::External, "external"),
    ] {
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, format!("\"{serialized}\""));
        assert_eq!(
            serde_json::from_str::<CliIntegrationState>(&json).unwrap(),
            state
        );
    }
}

#[test]
fn test_snippet_insertion_and_removal() {
    let content = "export PATH=$PATH:~/.local/bin\n";
    let snippet = format!(
        "{CLI_MARKER_BEGIN}\nexport CLOUD_CODE_URL=\"http://127.0.0.1:51234\"\n{CLI_MARKER_END}\n"
    );

    let updated = format!("{content}\n{snippet}");
    assert!(updated.contains(CLI_MARKER_BEGIN));
    assert!(updated.contains("CLOUD_CODE_URL=\"http://127.0.0.1:51234\""));

    let cleaned = patch::remove_snippet_lines(&updated, false);
    assert!(!cleaned.contains(CLI_MARKER_BEGIN));
    assert!(!cleaned.contains("CLOUD_CODE_URL"));
    assert!(cleaned.contains("export PATH=$PATH:~/.local/bin"));
}

#[test]
fn test_last_shell_assignment_wins() {
    let content = format!(
        "export CLOUD_CODE_URL=\"https://external.example\"\n{CLI_MARKER_BEGIN}\nexport CLOUD_CODE_URL=\"http://127.0.0.1:51234\"\n{CLI_MARKER_END}\n",
    );
    assert_eq!(
        patch::extract_endpoint_from_content(&content),
        Some("http://127.0.0.1:51234".to_string())
    );
}

#[test]
fn test_enable_and_disable_cli_integration() {
    let temp_dir = TempDir::new().unwrap();
    let zshrc = temp_dir.path().join(".zshrc");
    fs::write(&zshrc, "# User zshrc\n").unwrap();

    let endpoint = "http://127.0.0.1:51234";
    let is_fish = false;
    let snippet =
        format!("{CLI_MARKER_BEGIN}\nexport CLOUD_CODE_URL=\"{endpoint}\"\n{CLI_MARKER_END}\n");
    patch::update_shell_config_file(&zshrc, &snippet, is_fish).unwrap();

    let read_back = fs::read_to_string(&zshrc).unwrap();
    assert!(read_back.contains(CLI_MARKER_BEGIN));
    assert_eq!(
        patch::extract_endpoint_from_content(&read_back),
        Some(endpoint.to_string())
    );

    patch::remove_snippet_from_file(&zshrc, is_fish).unwrap();
    let cleaned_back = fs::read_to_string(&zshrc).unwrap();
    assert!(!cleaned_back.contains(CLI_MARKER_BEGIN));
}

#[test]
fn test_enable_rolls_back_when_ownership_write_fails() {
    let temp_dir = TempDir::new().unwrap();
    let integration_root = temp_dir.path().join("integration");
    let zshrc = temp_dir.path().join(".zshrc");
    let bashrc = temp_dir.path().join(".bashrc");
    let zshrc_before = b"# User zshrc\n";
    let bashrc_before = b"# User bashrc\n";
    fs::write(&zshrc, zshrc_before).unwrap();
    fs::write(&bashrc, bashrc_before).unwrap();

    fs::create_dir_all(&integration_root).unwrap();
    let ownership_path = integration_root.join(CLI_OWNERSHIP_FILE);
    fs::create_dir(&ownership_path).unwrap();

    let result = super::transaction::enable_cli_integration_with_target_files(
        &integration_root,
        "http://127.0.0.1:51234",
        &[zshrc.clone(), bashrc.clone()],
    );

    assert!(result.is_err());
    assert_eq!(fs::read(&zshrc).unwrap(), zshrc_before);
    assert_eq!(fs::read(&bashrc).unwrap(), bashrc_before);
    assert!(!integration_root
        .join("cli-integration")
        .join("env.sh")
        .exists());
    assert!(ownership_path.is_dir());
}
