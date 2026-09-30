use context_harness::config::{load_config, Config, McpServerConfig};

const BASE: &str = r#"
[db]
path = ".ctx/data/ctx.sqlite"
[chunking]
max_tokens = 700
[retrieval]
final_limit = 12
[server]
bind = "127.0.0.1:0"
"#;

fn server() -> McpServerConfig {
    toml::from_str("command = 'nonexistent-mcp-command'").unwrap()
}

#[test]
fn server_defaults_and_round_trip_are_stable() {
    assert!(Config::minimal().mcp_servers.is_empty());
    let config: Config = toml::from_str(BASE).unwrap();
    assert!(config.mcp_servers.is_empty());
    let server = server();
    assert!(server.args.is_empty());
    assert_eq!(server.timeout_seconds, 30);
    server.validate("docs_Server-1").unwrap();
    let value = serde_json::to_value(&server).unwrap();
    let restored: McpServerConfig = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap(), value);
}

#[test]
fn server_schema_rejects_unknown_fields_and_wrong_types() {
    for extra in [
        "env = { SECRET = 'value' }",
        "cwd = '/tmp'",
        "transport = 'http'",
        "timeout_seconds = -1",
        "timeout_seconds = '30'",
        "args = 'argument'",
        "args = [1]",
    ] {
        assert!(
            toml::from_str::<McpServerConfig>(&format!("command = 'mcp'\n{extra}")).is_err(),
            "accepted {extra}"
        );
    }
    assert!(toml::from_str::<McpServerConfig>("args = []").is_err());
}

#[test]
fn namespaces_and_process_settings_enforce_bounds() {
    let mut server = server();
    for name in [
        "",
        "two.names",
        "slash/name",
        "with space",
        "café",
        &"a".repeat(65),
    ] {
        assert!(server.validate(name).is_err(), "accepted {name:?}");
    }
    server.validate(&"a".repeat(64)).unwrap();
    for command in [
        "".to_owned(),
        "  ".to_owned(),
        "a\0b".to_owned(),
        "a".repeat(4097),
    ] {
        server.command = command;
        assert!(server.validate("docs").is_err());
    }
    server.command = "a".repeat(4096);
    server.validate("docs").unwrap();
    for args in [
        vec!["".into(); 129],
        vec!["a\0b".into()],
        vec!["a".repeat(65537)],
    ] {
        server.args = args;
        assert!(server.validate("docs").is_err());
    }
    server.args = vec!["a".repeat(512); 128];
    server.validate("docs").unwrap();
    for timeout in [0, 61, u64::MAX] {
        server.timeout_seconds = timeout;
        assert!(server.validate("docs").is_err());
    }
    for timeout in [1, 60] {
        server.timeout_seconds = timeout;
        server.validate("docs").unwrap();
    }
}

#[test]
fn normal_config_loading_validates_every_definition_without_launching() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.toml");
    std::fs::write(
        &path,
        format!("{BASE}\n[mcp_servers.docs]\ncommand = 'does-not-exist'\n"),
    )
    .unwrap();
    let config = load_config(&path).unwrap();
    assert_eq!(config.mcp_servers.len(), 1);
    assert_eq!(config.mcp_servers["docs"].timeout_seconds, 30);
    for definition in [
        "[mcp_servers.unused]\ncommand = ''",
        "[mcp_servers.unused]\ncommand = 'mcp'\ntimeout_seconds = 0",
        "[mcp_servers.'invalid.name']\ncommand = 'mcp'",
        "[mcp_servers.unused]\ncommand = 'mcp'\nenv = {}",
    ] {
        std::fs::write(
            &path,
            format!("{BASE}\n[mcp_servers.docs]\ncommand = 'does-not-exist'\n{definition}\n"),
        )
        .unwrap();
        assert!(load_config(&path).is_err(), "accepted {definition}");
    }
}
