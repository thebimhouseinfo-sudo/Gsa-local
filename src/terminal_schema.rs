use crate::ollama::ToolDefinition;
use schemars::JsonSchema;
use serde_json::Value;

pub fn model_schema<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).expect("JsonSchema root must serialize to JSON")
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
        assert!(parsed.defaulted.is_empty());
        assert!(parsed.optional.is_none());
    }
}
