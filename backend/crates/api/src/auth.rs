//! Cognito access-token verification at the HTTP trust boundary.
use std::{collections::HashMap, sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub struct CognitoConfig { pub issuer: String, pub app_client_id: String }
impl CognitoConfig {
    pub fn from_env() -> Result<Self, AuthError> {
        let region = std::env::var("AWS_REGION").map_err(|_| AuthError::Unavailable)?;
        let pool = std::env::var("COGNITO_USER_POOL_ID").map_err(|_| AuthError::Unavailable)?;
        let client = std::env::var("COGNITO_APP_CLIENT_ID").map_err(|_| AuthError::Unavailable)?;
        if region.trim().is_empty() || pool.trim().is_empty() || client.trim().is_empty()
            || pool.split('_').next().unwrap_or_default() != region {
            return Err(AuthError::Unavailable);
        }
        Ok(Self { issuer: format!("https://cognito-idp.{region}.amazonaws.com/{pool}"), app_client_id: client })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedPrincipal { pub subject: String }
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthError { InvalidToken, Unavailable }
#[derive(Clone, Debug, Deserialize)]
struct Claims {
    sub: String, iss: String, exp: u64, iat: u64, token_use: String, client_id: String,
    #[serde(default)] nbf: Option<u64>,
}
#[derive(Clone, Debug, Deserialize)]
struct Jwks { keys: Vec<Jwk> }
#[derive(Clone, Debug, Deserialize)]
struct Jwk {
    kid: String, kty: String, alg: Option<String>,
    #[serde(rename="use")] key_use: Option<String>,
    n: String, e: String,
}
#[derive(Clone)]
pub struct CognitoTokenVerifier { config: CognitoConfig, client: reqwest::Client, cache: Arc<RwLock<KeyCache>> }
#[derive(Default)]
struct KeyCache { keys: HashMap<String, DecodingKey>, expires_at: u64 }

impl CognitoTokenVerifier {
    pub fn new(config: CognitoConfig) -> Result<Self, AuthError> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(3)).build()
            .map_err(|_| AuthError::Unavailable)?;
        Ok(Self { config, client, cache: Arc::new(RwLock::new(KeyCache::default())) })
    }
    async fn key_for(&self, kid: &str) -> Result<DecodingKey, AuthError> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| AuthError::Unavailable)?.as_secs();
        {
            let cache = self.cache.read().await;
            if cache.expires_at > now {
                if let Some(key) = cache.keys.get(kid) { return Ok(key.clone()); }
            }
        }
        let url = format!("{}/.well-known/jwks.json", self.config.issuer);
        let jwks: Jwks = self.client.get(url).send().await.map_err(|_| AuthError::Unavailable)?
            .error_for_status().map_err(|_| AuthError::Unavailable)?
            .json().await.map_err(|_| AuthError::Unavailable)?;
        let mut keys = HashMap::new();
        for key in jwks.keys {
            if key.kty == "RSA" && key.alg.as_deref().unwrap_or("RS256") == "RS256"
                && key.key_use.as_deref().unwrap_or("sig") == "sig" {
                if let Ok(decoded) = DecodingKey::from_rsa_components(&key.n, &key.e) {
                    keys.insert(key.kid, decoded);
                }
            }
        }
        if keys.is_empty() { return Err(AuthError::Unavailable); }
        let mut cache = self.cache.write().await;
        cache.keys = keys;
        cache.expires_at = now.saturating_add(300);
        cache.keys.get(kid).cloned().ok_or(AuthError::InvalidToken)
    }
    pub async fn verify(&self, token: &str) -> Result<VerifiedPrincipal, AuthError> {
        let header = decode_header(token).map_err(|_| AuthError::InvalidToken)?;
        if header.alg != Algorithm::RS256 { return Err(AuthError::InvalidToken); }
        let kid = header.kid.ok_or(AuthError::InvalidToken)?;
        let key = self.key_for(&kid).await?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[self.config.issuer.as_str()]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.leeway = 30;
        validation.required_spec_claims.clear();
        let c = decode::<Claims>(token, &key, &validation).map_err(|_| AuthError::InvalidToken)?.claims;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| AuthError::Unavailable)?.as_secs();
        if c.iss != self.config.issuer || c.exp <= now || c.iat > now.saturating_add(30)
            || c.nbf.is_some_and(|v| v > now.saturating_add(30))
            || c.token_use != "access" || c.client_id != self.config.app_client_id || c.sub.trim().is_empty() {
            return Err(AuthError::InvalidToken);
        }
        Ok(VerifiedPrincipal { subject: c.sub })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cognito_config_rejects_empty_fields() {
        assert!(CognitoConfig { issuer: String::new(), app_client_id: String::new() }.issuer.is_empty());
        assert_eq!(AuthError::InvalidToken, AuthError::InvalidToken);
    }
}
