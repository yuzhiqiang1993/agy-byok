use serde::{Deserialize, Deserializer};

pub(crate) fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Contract {
        #[serde(deserialize_with = "required_nullable")]
        value: Option<String>,
    }

    #[test]
    fn nullable_contract_requires_the_key_and_rejects_unknown_fields() {
        let explicit_null: Contract = serde_json::from_str(r#"{"value":null}"#).unwrap();

        assert!(explicit_null.value.is_none());
        assert!(serde_json::from_str::<Contract>("{}").is_err());
        assert!(serde_json::from_str::<Contract>(r#"{"value":null,"typo":true}"#).is_err());
    }
}
