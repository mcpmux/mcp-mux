//! Feature conversion - MCP protocol types to ServerFeature

use mcpmux_core::{FeatureType, ServerFeature};
use rmcp::model::{Prompt, Resource, Tool};

/// Trait for converting MCP protocol types to ServerFeature (DRY + OCP)
pub trait ToServerFeature {
    fn name(&self) -> String;
    fn description(&self) -> Option<String>;
    fn to_raw_json(&self) -> Option<serde_json::Value>;
    fn feature_type() -> FeatureType;
}

impl ToServerFeature for Tool {
    fn name(&self) -> String {
        self.name.to_string()
    }

    fn description(&self) -> Option<String> {
        self.description.as_ref().map(|d| d.to_string())
    }

    fn to_raw_json(&self) -> Option<serde_json::Value> {
        serde_json::to_value(self).ok()
    }

    fn feature_type() -> FeatureType {
        FeatureType::Tool
    }
}

impl ToServerFeature for Prompt {
    fn name(&self) -> String {
        self.name.to_string()
    }

    fn description(&self) -> Option<String> {
        self.description.as_ref().map(|d| d.to_string())
    }

    fn to_raw_json(&self) -> Option<serde_json::Value> {
        serde_json::to_value(self).ok()
    }

    fn feature_type() -> FeatureType {
        FeatureType::Prompt
    }
}

/// Generic conversion function (DRY)
pub fn convert_to_feature<T: ToServerFeature>(
    space_id: &str,
    server_id: &str,
    item: T,
) -> ServerFeature {
    let name = item.name();
    let raw_json = item.to_raw_json();

    let mut feature = match T::feature_type() {
        FeatureType::Tool => ServerFeature::tool(space_id, server_id, &name),
        FeatureType::Prompt => ServerFeature::prompt(space_id, server_id, &name),
        FeatureType::Resource => ServerFeature::resource(space_id, server_id, &name),
    };

    if let Some(desc) = item.description() {
        feature = feature.with_description(desc);
    }
    if let Some(json) = raw_json {
        feature = feature.with_raw_json(json);
    }

    feature
}

/// Resource needs special handling (nested .raw structure + dual naming)
pub fn resource_to_feature(space_id: &str, server_id: &str, resource: Resource) -> ServerFeature {
    let uri = resource.raw.uri.clone();
    let raw_json = serde_json::to_value(&resource.raw).ok();

    let mut feature = ServerFeature::resource(space_id, server_id, &uri);
    if !resource.raw.name.is_empty() {
        feature = feature.with_display_name(resource.raw.name.clone());
    }
    if let Some(desc) = &resource.raw.description {
        feature = feature.with_description(desc.clone());
    }
    if let Some(json) = raw_json {
        feature = feature.with_raw_json(json);
    }
    feature
}
