use crate::ollama::ToolDefinition;
use schemars::{r#gen::SchemaSettings, JsonSchema};
use serde_json::Value;

pub fn model_schema<T: JsonSchema>() -> Value {
    // Ollama's underlying constrained tool-schema parser can reject otherwise
    // valid nested #/definitions/$ref links. Generate a self-contained schema
    // at the common typed terminal-tool boundary instead of weakening only the
    // Tester schema or hardcoding one terminal payload.
    let settings = SchemaSettings::draft07().with(|settings| {
        settings.inline_subschemas = true;
    });
    let schema = settings.into_generator().into_root_schema_for::<T>();
    let schema = serde_json::to_value(schema).expect("JsonSchema root must serialize to JSON");
    // Recursive types can still contain $ref despite inline_subschemas. Do not
    // send an unresolved reference to Ollama and accept an unexplained HTTP 500.
    assert!(
        !contains_json_ref(&schema),
        "typed terminal tool schema contains a recursive/unresolved $ref; provide a nonrecursive DTO"
    );
    schema
}

fn contains_json_ref(schema: &Value) -> bool {
    match schema {
        Value::Object(fields) => {
            fields.contains_key("$ref") || fields.values().any(contains_json_ref)
        }
        Value::Array(items) => items.iter().any(contains_json_ref),
        _ => false,
    }
}

pub fn typed_terminal_tool<T: JsonSchema>(
    name: impl Into<String>,
    description: impl Into<String>,
) -> ToolDefinition {
    ToolDefinition::function(name, description, model_schema::<T>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, JsonSchema)]
    struct Fixture {
        required: String,
        #[serde(default)]
        defaulted: Vec<String>,
        optional: Option<String>,
    }

    #[derive(JsonSchema)]
    struct NestedChild {
        status: NestedStatus,
    }

    #[derive(JsonSchema)]
    enum NestedStatus {
        Ready,
        Blocked,
    }

    #[derive(JsonSchema)]
    struct NestedFixture {
        children: Vec<NestedChild>,
        #[serde(default)]
        optional_children: Vec<NestedChild>,
    }

    #[test]
    fn exported_schema_inlines_nested_definitions_instead_of_sending_ollama_refs() {
        let original = serde_json::to_value(schemars::schema_for!(NestedFixture)).unwrap();
        assert!(
            contains_json_ref(&original),
            "negative control must reproduce a reference-bearing schema"
        );

        let exported = model_schema::<NestedFixture>();
        assert!(!contains_json_ref(&exported));
        assert!(exported["definitions"].is_null());
        assert_eq!(exported["properties"]["children"]["type"], "array");
        assert_eq!(
            exported["properties"]["children"]["items"]["properties"]["status"]["enum"],
            serde_json::json!(["Ready", "Blocked"])
        );
        let required = exported["required"].as_array().unwrap();
        assert!(required.iter().any(|field| field == "children"));
        assert!(!required.iter().any(|field| field == "optional_children"));
    }

    #[test]
    fn generated_schema_tracks_serde_presence_semantics() {
        let schema = model_schema::<Fixture>();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|field| field == "required"));
        assert!(!required.iter().any(|field| field == "defaulted"));
        assert!(!required.iter().any(|field| field == "optional"));

        let parsed: Fixture = serde_json::from_value(serde_json::json!({
            "required":"value"
        }))
        .unwrap();
        assert_eq!(parsed.required, "value");
        assert!(parsed.defaulted.is_empty());
        assert!(parsed.optional.is_none());
    }
}
