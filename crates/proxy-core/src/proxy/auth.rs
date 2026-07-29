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

    pub fn get_token(&self) -> &str {
        &self.token
    }

    pub fn validate_header(&self, auth_header: Option<&str>) -> bool {
        match auth_header {
            Some(header) => {
                let token_part = header.trim_start_matches("Bearer ").trim();
                token_part == self.token
            }
            None => false,
        }
    }
}
