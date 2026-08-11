mod checkpoint;
mod custom;

use serde_json::Value;

pub struct AntigravityModelDescriptor;

impl AntigravityModelDescriptor {
    pub fn model_count(payload: &Value) -> usize {
        match catalog_models(payload) {
            Value::Object(models) => models.len(),
            Value::Array(models) => models.len(),
            _ => 0,
        }
    }
}

fn catalog_container(payload: &Value) -> &Value {
    if payload.get("models").is_some() {
        payload
    } else {
        payload
            .get("response")
            .filter(|response| response.get("models").is_some())
            .unwrap_or(payload)
    }
}

fn catalog_container_mut(payload: &mut Value) -> &mut Value {
    if payload.get("models").is_some() {
        return payload;
    }
    if payload
        .get("response")
        .is_some_and(|response| response.get("models").is_some())
    {
        return payload
            .get_mut("response")
            .expect("checked response catalog must exist");
    }
    payload
}

fn catalog_models(payload: &Value) -> &Value {
    let container = catalog_container(payload);
    container.get("models").unwrap_or(container)
}

fn catalog_models_mut(payload: &mut Value) -> &mut Value {
    let container = catalog_container_mut(payload);
    if container.get("models").is_some() {
        container
            .get_mut("models")
            .expect("checked model catalog must exist")
    } else {
        container
    }
}

#[cfg(test)]
mod tests;
