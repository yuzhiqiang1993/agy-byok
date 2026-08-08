use uuid::Uuid;

#[derive(Clone)]
pub struct AuthManager {
    token: String,
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthManager {
    pub fn new() -> Self {
        Self {
            token: Uuid::new_v4().to_string(),
        }
    }

    #[cfg(test)]
    pub fn get_token(&self) -> &str {
        &self.token
    }

    pub fn validate_header(&self, auth_header: Option<&str>) -> bool {
        auth_header
            .and_then(|header| header.strip_prefix("Bearer "))
            .is_some_and(|token| self.validate_token(token))
    }

    pub fn validate_token(&self, token: &str) -> bool {
        token == self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_requires_exact_bearer_scheme_and_token() {
        let auth = AuthManager::new();
        let valid_header = format!("Bearer {}", auth.get_token());

        assert!(auth.validate_header(Some(&valid_header)));
        assert!(auth.validate_token(auth.get_token()));
        assert!(!auth.validate_header(Some(auth.get_token())));
        assert!(!auth.validate_header(Some("bearer invalid")));
        assert!(!auth.validate_header(None));
    }
}
