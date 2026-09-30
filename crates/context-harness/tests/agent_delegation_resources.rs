use context_harness::agent_resource::{AgentResource, Capability};

const RESOURCE: &str = r#"
[agent]
name = "parent"
model = "reasoning"
tools = []
[prompt]
system = "Use project context."
"#;

fn delegation(allow: &[&str]) -> String {
    format!(
        "{}\n[agent.delegation]\nallow = {}\n",
        RESOURCE.replace("tools = []", "tools = ['agent.invoke']"),
        serde_json::to_string(allow).unwrap()
    )
}

#[test]
fn absent_and_empty_delegation_preserve_pre_delegation_versions() {
    for suffix in [
        "",
        "\n[agent.delegation]",
        "\n[agent.delegation]\nallow = []",
    ] {
        let resource = AgentResource::parse(&format!("{RESOURCE}{suffix}")).unwrap();
        assert!(resource.agent.delegation.allow.is_empty());
        assert_eq!(
            resource.version().unwrap(),
            "sha256:ca55fce8816852d93a4d62ae4ac9891ea01c215aebe7b0bc3036e0f526589f81"
        );
        assert!(serde_json::to_value(resource).unwrap()["agent"]
            .get("delegation")
            .is_none());
    }
}

#[test]
fn explicit_delegation_round_trips_and_changes_version() {
    let resource = AgentResource::parse(&delegation(&["researcher", "code-review.v2"])).unwrap();
    assert_eq!(
        resource.agent.delegation.allow,
        ["researcher", "code-review.v2"]
    );
    let encoded = toml::to_string(&resource).unwrap();
    let round_trip = AgentResource::parse(&encoded).unwrap();
    assert_eq!(resource.version().unwrap(), round_trip.version().unwrap());
    let different = AgentResource::parse(&delegation(&["reviewer"])).unwrap();
    assert_ne!(resource.version().unwrap(), different.version().unwrap());
}

#[test]
fn delegation_rejects_invalid_targets_unknown_settings_and_missing_tool() {
    let too_many: Vec<String> = (0..33).map(|n| format!("child{n}")).collect();
    let references: Vec<&str> = too_many.iter().map(String::as_str).collect();
    for invalid in [
        delegation(&["parent"]),
        delegation(&["child", "child"]),
        delegation(&[""]),
        delegation(&["../child"]),
        delegation(&["child name"]),
        delegation(&["chíld"]),
        delegation(&[]),
        delegation(&references),
        delegation(&["child"]).replace("tools = ['agent.invoke']", "tools = []"),
        format!("{}\nmax_depth = 4", delegation(&["child"])),
        format!("{}\nallow_all = true", delegation(&["child"])),
        RESOURCE.replace("tools = []", "tools = ['agent.invoke']"),
    ] {
        assert!(
            AgentResource::parse(&invalid).is_err(),
            "accepted {invalid}"
        );
    }
    assert!(AgentResource::parse(&delegation(&references[..32])).is_ok());
}

#[test]
fn delegation_capability_requires_explicit_agent_permission() {
    let read_only = AgentResource::parse(&delegation(&["child"])).unwrap();
    assert_eq!(
        read_only.agent.permissions.allowed(),
        [Capability::ReadOnly]
    );
    let explicit = AgentResource::parse(&format!(
        "{}\n[agent.permissions]\nallow = ['read_only', 'agent_delegate']",
        delegation(&["child"])
    ))
    .unwrap();
    assert_eq!(
        explicit.agent.permissions.allowed(),
        [Capability::ReadOnly, Capability::AgentDelegate]
    );
}
