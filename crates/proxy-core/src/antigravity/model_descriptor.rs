use crate::domain::{UpstreamModel, VirtualModel};
use serde_json::{json, Value};

pub struct AntigravityModelDescriptor;

impl AntigravityModelDescriptor {
    pub fn build_model_object(
        virtual_model: &VirtualModel,
        upstream_model: &UpstreamModel,
    ) -> Value {
        let caps = &upstream_model.capabilities;
        let mut supported_mime_types = vec!["text/plain", "text/markdown"];
        if caps.vision {
            supported_mime_types.push("image/png");
            supported_mime_types.push("image/jpeg");
            supported_mime_types.push("image/webp");
        }

        json!({
            "id": virtual_model.id,
            "name": format!("models/{}", virtual_model.id),
            "displayName": virtual_model.display_name,
            "description": format!("Custom BYOK Model (Provider: {})", upstream_model.provider_id),
            "inputTokenLimit": 128000,
            "outputTokenLimit": 8192,
            "supportsImages": caps.vision,
            "supportsTools": caps.tools,
            "supportsThinking": caps.thinking,
            "supportedMimeTypes": supported_mime_types
        })
    }

    pub fn inject_into_model_list(
        models_json: &mut Value,
        virtual_models: &[VirtualModel],
        upstream_models: &[UpstreamModel],
    ) {
        let descriptors: Vec<Value> = virtual_models
            .iter()
            .filter(|vm| vm.enabled)
            .filter_map(|vm| {
                upstream_models
                    .iter()
                    .find(|um| um.id == vm.upstream_model_id && um.enabled)
                    .map(|um| Self::build_model_object(vm, um))
            })
            .collect();

        if let Some(arr) = models_json["models"].as_array_mut() {
            for desc in descriptors {
                arr.push(desc);
            }
        } else if let Some(arr) = models_json.as_array_mut() {
            for desc in descriptors {
                arr.push(desc);
            }
        } else if let Some(obj) = models_json.as_object_mut() {
            for desc in descriptors {
                if let Some(id) = desc["id"].as_str() {
                    obj.insert(id.to_string(), desc);
                }
            }
        }
    }
}
