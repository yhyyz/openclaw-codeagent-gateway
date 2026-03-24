//! Token-based identity resolution.

use crate::error::Error;

pub fn extract_bearer_token(auth_header: &str) -> Result<&str, Error> {
    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or(Error::Unauthorized)?;
    if token.is_empty() {
        return Err(Error::Unauthorized);
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_valid_bearer() {
        let token = extract_bearer_token("Bearer abc123").unwrap();
        assert_eq!(token, "abc123");
    }

    #[test]
    fn test_extract_valid_bearer_with_special_chars() {
        let token = extract_bearer_token("Bearer tok_ops-primary.v2").unwrap();
        assert_eq!(token, "tok_ops-primary.v2");
    }

    #[test]
    fn test_extract_missing_prefix() {
        let err = extract_bearer_token("Basic abc").unwrap_err();
        assert!(matches!(err, Error::Unauthorized));
    }

    #[test]
    fn test_extract_empty_token() {
        let err = extract_bearer_token("Bearer ").unwrap_err();
        assert!(matches!(err, Error::Unauthorized));
    }

    #[test]
    fn test_extract_no_space_after_bearer() {
        let err = extract_bearer_token("Bearertoken").unwrap_err();
        assert!(matches!(err, Error::Unauthorized));
    }
}
