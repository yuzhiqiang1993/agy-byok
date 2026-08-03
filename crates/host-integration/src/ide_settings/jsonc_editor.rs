use super::{settings_conflict, IDE_CLOUD_CODE_SETTING};
use crate::error::HostIntegrationError;
use serde_json::Value;

pub(super) fn cloud_code_value(bytes: &[u8]) -> Result<Option<Value>, HostIntegrationError> {
    let source = std::str::from_utf8(bytes)
        .map_err(|_| settings_conflict("IDE settings file is not valid UTF-8"))?;
    let value: Value = json5::from_str(source).map_err(|error| {
        settings_conflict(format!("failed to parse IDE JSONC settings: {error}"))
    })?;
    let object = value
        .as_object()
        .ok_or_else(|| settings_conflict("IDE settings root must be an object"))?;
    Ok(object.get(IDE_CLOUD_CODE_SETTING).cloned())
}

fn settings_root_object(bytes: &[u8]) -> Result<RootObject, HostIntegrationError> {
    let source = std::str::from_utf8(bytes)
        .map_err(|_| settings_conflict("IDE settings file is not valid UTF-8"))?;
    ensure_unique_cloud_code_property(source)
}

pub(super) fn settings_trailing_comma(bytes: &[u8]) -> Result<bool, HostIntegrationError> {
    Ok(settings_root_object(bytes)?.trailing_comma)
}

fn ensure_unique_cloud_code_property(source: &str) -> Result<RootObject, HostIntegrationError> {
    let object = scan_root_object(source)?;
    if object
        .properties
        .iter()
        .filter(|property| property.key == IDE_CLOUD_CODE_SETTING)
        .count()
        > 1
    {
        return Err(settings_conflict(
            "IDE settings contains duplicate jetski.cloudCodeUrl keys",
        ));
    }
    Ok(object)
}

pub(super) fn configure_settings(
    bytes: &[u8],
    endpoint: &str,
) -> Result<Vec<u8>, HostIntegrationError> {
    configure_setting_value(bytes, &Value::String(endpoint.to_string()))
}

pub(super) fn configure_setting_value(
    bytes: &[u8],
    configured_value: &Value,
) -> Result<Vec<u8>, HostIntegrationError> {
    let source = std::str::from_utf8(bytes)
        .map_err(|_| settings_conflict("IDE settings file is not valid UTF-8"))?;
    let value: Value = json5::from_str(source).map_err(|error| {
        settings_conflict(format!("failed to parse IDE JSONC settings: {error}"))
    })?;
    if !value.is_object() {
        return Err(settings_conflict("IDE settings root must be an object"));
    }

    let object = ensure_unique_cloud_code_property(source)?;
    let matches = object
        .properties
        .iter()
        .filter(|property| property.key == IDE_CLOUD_CODE_SETTING)
        .collect::<Vec<_>>();
    let encoded_value =
        serde_json::to_string(configured_value).expect("JSON value serialization cannot fail");
    let configured = if let Some(property) = matches.first() {
        format!(
            "{}{}{}",
            &source[..property.value_start],
            encoded_value,
            &source[property.value_end..]
        )
    } else if let Some(last_prop) = object.properties.last() {
        if let Some(comma_after) = last_prop.comma_after {
            format!(
                "{}\n  {}: {}{}",
                &source[..=comma_after],
                serde_json::to_string(IDE_CLOUD_CODE_SETTING).unwrap(),
                encoded_value,
                &source[comma_after + 1..]
            )
        } else {
            format!(
                "{},\n  {}: {}{}",
                &source[..last_prop.value_end],
                serde_json::to_string(IDE_CLOUD_CODE_SETTING).unwrap(),
                encoded_value,
                &source[last_prop.value_end..]
            )
        }
    } else {
        let insertion = format!(
            "\n  {}: {}\n",
            serde_json::to_string(IDE_CLOUD_CODE_SETTING).unwrap(),
            encoded_value
        );
        format!(
            "{}{}{}",
            &source[..object.close_brace],
            insertion,
            &source[object.close_brace..]
        )
    };
    if cloud_code_value(configured.as_bytes())?.as_ref() != Some(configured_value) {
        return Err(settings_conflict(
            "configured IDE settings did not retain the requested value",
        ));
    }
    Ok(configured.into_bytes())
}

pub(super) fn remove_setting(
    bytes: &[u8],
    retain_preceding_comma: bool,
) -> Result<Vec<u8>, HostIntegrationError> {
    let source = std::str::from_utf8(bytes)
        .map_err(|_| settings_conflict("IDE settings file is not valid UTF-8"))?;
    let value: Value = json5::from_str(source).map_err(|error| {
        settings_conflict(format!("failed to parse IDE JSONC settings: {error}"))
    })?;
    if !value.is_object() {
        return Err(settings_conflict("IDE settings root must be an object"));
    }
    let object = ensure_unique_cloud_code_property(source)?;
    let Some(property) = object
        .properties
        .iter()
        .find(|property| property.key == IDE_CLOUD_CODE_SETTING)
    else {
        return Ok(bytes.to_vec());
    };

    let (remove_start, remove_end) = match (property.comma_before, property.comma_after) {
        (_, Some(comma_after)) => (property.property_start, comma_after + 1),
        (Some(_), None) if retain_preceding_comma => (property.property_start, property.value_end),
        (Some(comma_before), None) => (comma_before, property.value_end),
        (None, None) => (property.property_start, property.value_end),
    };
    let updated = format!("{}{}", &source[..remove_start], &source[remove_end..]);
    if cloud_code_value(updated.as_bytes())?.is_some() {
        return Err(settings_conflict(
            "updated IDE settings still contains jetski.cloudCodeUrl",
        ));
    }
    Ok(updated.into_bytes())
}

struct RootObject {
    close_brace: usize,
    properties: Vec<JsonProperty>,
    trailing_comma: bool,
}

#[derive(Debug)]
struct JsonProperty {
    key: String,
    property_start: usize,
    value_start: usize,
    value_end: usize,
    comma_before: Option<usize>,
    comma_after: Option<usize>,
}

fn scan_root_object(source: &str) -> Result<RootObject, HostIntegrationError> {
    let bytes = source.as_bytes();
    let mut index = 0;
    skip_trivia(bytes, &mut index)?;
    if bytes.get(index) != Some(&b'{') {
        return Err(settings_conflict(
            "IDE settings root must start with an object",
        ));
    }
    index += 1;
    let mut properties = Vec::new();
    let mut trailing_comma = false;
    let mut comma_before = None;

    loop {
        skip_trivia(bytes, &mut index)?;
        if bytes.get(index) == Some(&b'}') {
            return Ok(RootObject {
                close_brace: index,
                properties,
                trailing_comma,
            });
        }
        let key_start = index;
        let key_end = parse_string_end(bytes, index)?;
        let key: String = serde_json::from_str(&source[key_start..key_end]).map_err(|error| {
            settings_conflict(format!("invalid quoted IDE settings key: {error}"))
        })?;
        index = key_end;
        skip_trivia(bytes, &mut index)?;
        if bytes.get(index) != Some(&b':') {
            return Err(settings_conflict("IDE settings property is missing ':'"));
        }
        index += 1;
        skip_trivia(bytes, &mut index)?;
        let value_start = index;
        let value_end = parse_value_end(bytes, index)?;
        index = value_end;
        skip_trivia(bytes, &mut index)?;
        let comma_after = match bytes.get(index) {
            Some(b',') => {
                let comma_after = index;
                index += 1;
                skip_trivia(bytes, &mut index)?;
                trailing_comma = bytes.get(index) == Some(&b'}');
                Some(comma_after)
            }
            Some(b'}') => {
                trailing_comma = false;
                None
            }
            _ => {
                return Err(settings_conflict(
                    "IDE settings property is missing a comma or closing brace",
                ))
            }
        };
        properties.push(JsonProperty {
            key,
            property_start: key_start,
            value_start,
            value_end,
            comma_before,
            comma_after,
        });
        if trailing_comma || comma_after.is_none() {
            continue;
        }
        comma_before = comma_after;
    }
}

fn parse_value_end(bytes: &[u8], start: usize) -> Result<usize, HostIntegrationError> {
    match bytes.get(start) {
        Some(b'"') => parse_string_end(bytes, start),
        Some(b'{') | Some(b'[') => parse_composite_end(bytes, start),
        Some(_) => {
            let mut index = start;
            while let Some(byte) = bytes.get(index) {
                if byte.is_ascii_whitespace() || matches!(byte, b',' | b'}' | b']') {
                    break;
                }
                if *byte == b'/' && matches!(bytes.get(index + 1), Some(b'/') | Some(b'*')) {
                    break;
                }
                index += 1;
            }
            if index == start {
                Err(settings_conflict(
                    "IDE settings property has an empty value",
                ))
            } else {
                Ok(index)
            }
        }
        None => Err(settings_conflict(
            "IDE settings property has an empty value",
        )),
    }
}

fn parse_composite_end(bytes: &[u8], start: usize) -> Result<usize, HostIntegrationError> {
    let mut stack = vec![if bytes[start] == b'{' { b'}' } else { b']' }];
    let mut index = start + 1;
    while let Some(byte) = bytes.get(index).copied() {
        match byte {
            b'"' => index = parse_string_end(bytes, index)?,
            b'/' if bytes.get(index + 1) == Some(&b'/') => skip_line_comment(bytes, &mut index),
            b'/' if bytes.get(index + 1) == Some(&b'*') => skip_block_comment(bytes, &mut index)?,
            b'{' => {
                stack.push(b'}');
                index += 1;
            }
            b'[' => {
                stack.push(b']');
                index += 1;
            }
            b'}' | b']' => {
                if stack.pop() != Some(byte) {
                    return Err(settings_conflict(
                        "IDE settings contains mismatched brackets",
                    ));
                }
                index += 1;
                if stack.is_empty() {
                    return Ok(index);
                }
            }
            _ => index += 1,
        }
    }
    Err(settings_conflict(
        "IDE settings contains an unterminated composite value",
    ))
}

fn parse_string_end(bytes: &[u8], start: usize) -> Result<usize, HostIntegrationError> {
    if bytes.get(start) != Some(&b'"') {
        return Err(settings_conflict(
            "IDE settings property keys must be quoted",
        ));
    }
    let mut index = start + 1;
    while let Some(byte) = bytes.get(index).copied() {
        match byte {
            b'\\' => {
                index += 2;
                if index > bytes.len() {
                    return Err(settings_conflict("IDE settings contains an invalid escape"));
                }
            }
            b'"' => return Ok(index + 1),
            _ => index += 1,
        }
    }
    Err(settings_conflict(
        "IDE settings contains an unterminated string",
    ))
}

fn skip_trivia(bytes: &[u8], index: &mut usize) -> Result<(), HostIntegrationError> {
    loop {
        while bytes.get(*index).is_some_and(u8::is_ascii_whitespace) {
            *index += 1;
        }
        if bytes.get(*index) == Some(&b'/') && bytes.get(*index + 1) == Some(&b'/') {
            skip_line_comment(bytes, index);
        } else if bytes.get(*index) == Some(&b'/') && bytes.get(*index + 1) == Some(&b'*') {
            skip_block_comment(bytes, index)?;
        } else {
            return Ok(());
        }
    }
}

fn skip_line_comment(bytes: &[u8], index: &mut usize) {
    *index += 2;
    while let Some(byte) = bytes.get(*index) {
        *index += 1;
        if *byte == b'\n' {
            break;
        }
    }
}

fn skip_block_comment(bytes: &[u8], index: &mut usize) -> Result<(), HostIntegrationError> {
    *index += 2;
    while *index + 1 < bytes.len() {
        if bytes[*index] == b'*' && bytes[*index + 1] == b'/' {
            *index += 2;
            return Ok(());
        }
        *index += 1;
    }
    Err(settings_conflict(
        "IDE settings contains an unterminated block comment",
    ))
}
