//! Registry validation against JSON Schema
//!
//! Note: JSON schemas are now maintained in the canonical `/schemas/` directory.
//! This module provides basic structural validation using serde deserialization.

/// Validate registry JSON against basic structure
///
/// This function provides minimal validation. Full schema validation should use
/// the canonical schemas in `/schemas/registry-bundle.schema.json`.
pub fn validate_registry_json(json: &str) -> Result<(), String> {
    // Parse JSON to ensure it's valid JSON
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("Invalid JSON: {}", e))?;

    // Basic structural validation
    if !value.is_object() {
        return Err("Registry must be a JSON object".to_string());
    }

    let obj = value.as_object().unwrap();

    // Check for required 'servers' field
    if !obj.contains_key("servers") {
        return Err("Missing required 'servers' field".to_string());
    }

    Ok(())
}
