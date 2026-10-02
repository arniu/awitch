/// The bearer token from `Authorization: Bearer <token>`.
///
/// RFC 7235: the scheme name is case-insensitive, and one or more spaces
/// separate it from the credentials.
pub(crate) fn bearer_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let (scheme, token) = value.split_once(char::is_whitespace)?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim_start())
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_a_bearer_credential() {
        for (value, expected) in [
            ("Bearer secret", Some("secret")),
            ("bearer secret", Some("secret")),
            ("Bearer  secret", Some("secret")),
            ("Basic secret", None),
        ] {
            let mut headers = axum::http::HeaderMap::new();
            headers.insert(axum::http::header::AUTHORIZATION, value.parse().unwrap());
            assert_eq!(super::bearer_token(&headers), expected, "{value}");
        }
    }
}
