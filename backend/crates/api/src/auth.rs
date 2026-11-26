//! Cognito access-token verification at the HTTP trust boundary.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use tokio::sync::{Mutex, RwLock};

#[derive(Clone, Debug)]
pub struct CognitoConfig {
    pub issuer: String,
    pub app_client_id: String,
}

impl CognitoConfig {
    /// Derive the issuer only from trusted deployment configuration.
    pub fn from_env() -> Result<Self, AuthError> {
        let region = std::env::var("AWS_REGION").map_err(|_| AuthError::Unavailable)?;
        let pool = std::env::var("COGNITO_USER_POOL_ID").map_err(|_| AuthError::Unavailable)?;
        let client = std::env::var("COGNITO_APP_CLIENT_ID").map_err(|_| AuthError::Unavailable)?;
        let region = region.trim().to_owned();
        let pool = pool.trim().to_owned();
        let client = client.trim().to_owned();

        if region.trim().is_empty()
            || pool.trim().is_empty()
            || client.trim().is_empty()
            || pool.split('_').next().unwrap_or_default() != region
        {
            return Err(AuthError::Unavailable);
        }

        Ok(Self {
            issuer: format!("https://cognito-idp.{region}.amazonaws.com/{pool}"),
            app_client_id: client,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedPrincipal {
    pub subject: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthError {
    InvalidToken,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize)]
struct Claims {
    sub: String,
    iss: String,
    exp: u64,
    iat: u64,
    token_use: String,
    client_id: String,
    #[serde(default)]
    nbf: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

#[derive(Clone, Debug, Deserialize)]
struct Jwk {
    kid: String,
    kty: String,
    alg: Option<String>,
    #[serde(rename = "use")]
    key_use: Option<String>,
    n: String,
    e: String,
}

#[derive(Clone)]
pub struct CognitoTokenVerifier {
    config: CognitoConfig,
    client: reqwest::Client,
    cache: Arc<RwLock<KeyCache>>,
    refresh_lock: Arc<Mutex<()>>,
    jwks_url: String,
}

#[derive(Default)]
struct KeyCache {
    keys: HashMap<String, DecodingKey>,
    expires_at: u64,
    last_refresh_at: Option<u64>,
    last_refresh_failed: bool,
}

impl CognitoTokenVerifier {
    pub fn new(config: CognitoConfig) -> Result<Self, AuthError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .map_err(|_| AuthError::Unavailable)?;

        let jwks_url = format!("{}/.well-known/jwks.json", config.issuer);
        Ok(Self {
            config,
            client,
            cache: Arc::new(RwLock::new(KeyCache::default())),
            refresh_lock: Arc::new(Mutex::new(())),
            jwks_url,
        })
    }

    #[cfg(test)]
    pub(crate) fn new_with_jwks_url(
        config: CognitoConfig,
        jwks_url: String,
    ) -> Result<Self, AuthError> {
        let mut verifier = Self::new(config)?;
        verifier.jwks_url = jwks_url;
        Ok(verifier)
    }

    async fn key_for(&self, kid: &str) -> Result<DecodingKey, AuthError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AuthError::Unavailable)?
            .as_secs();

        {
            let cache = self.cache.read().await;
            if cache.expires_at > now
                && let Some(key) = cache.keys.get(kid)
            {
                return Ok(key.clone());
            }
        }

        // Serialize refreshes and re-check after acquiring the lock (single-flight).
        let _refresh = self.refresh_lock.lock().await;
        {
            let cache = self.cache.read().await;
            if cache.expires_at > now
                && let Some(key) = cache.keys.get(kid)
            {
                return Ok(key.clone());
            }
            // Unknown kids are negatively cached by throttling refresh attempts globally.
            if cache
                .last_refresh_at
                .is_some_and(|last| now.saturating_sub(last) < 30)
            {
                return Err(if cache.last_refresh_failed {
                    AuthError::Unavailable
                } else {
                    AuthError::InvalidToken
                });
            }
        }

        // Record the attempt before I/O so failed requests are throttled too.
        {
            let mut cache = self.cache.write().await;
            cache.last_refresh_at = Some(now);
            cache.last_refresh_failed = true;
        }

        // Never follow jku/x5u supplied by an untrusted token header.
        let jwks: Jwks = self
            .client
            .get(&self.jwks_url)
            .send()
            .await
            .map_err(|_| AuthError::Unavailable)?
            .error_for_status()
            .map_err(|_| AuthError::Unavailable)?
            .json()
            .await
            .map_err(|_| AuthError::Unavailable)?;

        let mut keys = HashMap::new();
        for key in jwks.keys {
            if key.kty == "RSA"
                && key.alg.as_deref().unwrap_or("RS256") == "RS256"
                && key.key_use.as_deref().unwrap_or("sig") == "sig"
                && let Ok(decoded) = DecodingKey::from_rsa_components(&key.n, &key.e)
            {
                keys.insert(key.kid, decoded);
            }
        }

        if keys.is_empty() {
            return Err(AuthError::Unavailable);
        }

        let mut cache = self.cache.write().await;
        cache.keys = keys;
        cache.expires_at = now.saturating_add(300);
        cache.last_refresh_at = Some(now);
        cache.last_refresh_failed = false;
        cache.keys.get(kid).cloned().ok_or(AuthError::InvalidToken)
    }

    pub async fn verify(&self, token: &str) -> Result<VerifiedPrincipal, AuthError> {
        let header = decode_header(token).map_err(|_| AuthError::InvalidToken)?;
        if header.alg != Algorithm::RS256 {
            return Err(AuthError::InvalidToken);
        }

        let kid = header.kid.ok_or(AuthError::InvalidToken)?;
        let key = self.key_for(&kid).await?;

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[self.config.issuer.as_str()]);
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.leeway = 30;
        // Cognito access tokens use client_id, rather than aud, for app-client binding.
        validation.required_spec_claims.clear();

        let claims = decode::<Claims>(token, &key, &validation)
            .map_err(|_| AuthError::InvalidToken)?
            .claims;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AuthError::Unavailable)?
            .as_secs();

        if claims.iss != self.config.issuer
            || claims.exp <= now
            || claims.iat > now.saturating_add(30)
            || claims
                .nbf
                .is_some_and(|value| value > now.saturating_add(30))
            || claims.token_use != "access"
            || claims.client_id != self.config.app_client_id
            || claims.sub.trim().is_empty()
        {
            return Err(AuthError::InvalidToken);
        }

        Ok(VerifiedPrincipal {
            subject: claims.sub,
        })
    }
}

#[cfg(test)]
mod jwks_integration_tests {
    use super::*;
    use axum::{Json, extract::State, routing::get};
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use jsonwebtoken::{EncodingKey, Header, encode};
    use rand::rngs::OsRng;
    use rsa::{
        RsaPrivateKey,
        pkcs8::{EncodePrivateKey, LineEnding},
        traits::PublicKeyParts,
    };
    use serde_json::{Value, json};
    use tokio::{net::TcpListener, sync::RwLock, task::JoinHandle};

    struct Fixture {
        verifier: CognitoTokenVerifier,
        document: Arc<RwLock<Value>>,
        private_pem: String,
        server: JoinHandle<()>,
    }

    async fn serve_jwks(State(doc): State<Arc<RwLock<Value>>>) -> Json<Value> {
        Json(doc.read().await.clone())
    }

    async fn fixture() -> Fixture {
        let private = RsaPrivateKey::new(&mut OsRng, 2048).unwrap();
        let private_pem = private.to_pkcs8_pem(LineEnding::LF).unwrap().to_string();
        let n = URL_SAFE_NO_PAD.encode(private.n().to_bytes_be());
        let e = URL_SAFE_NO_PAD.encode(private.e().to_bytes_be());
        let document = Arc::new(RwLock::new(json!({"keys":[{
            "kid":"key-1","kty":"RSA","alg":"RS256","use":"sig","n":n,"e":e
        }]})));
        let app = axum::Router::new()
            .route("/.well-known/jwks.json", get(serve_jwks))
            .with_state(document.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let config = CognitoConfig {
            issuer: "https://issuer.example.test/pool".into(),
            app_client_id: "expected-client".into(),
        };
        let verifier = CognitoTokenVerifier::new_with_jwks_url(
            config,
            format!("http://{address}/.well-known/jwks.json"),
        )
        .unwrap();
        Fixture {
            verifier,
            document,
            private_pem,
            server,
        }
    }

    fn claims(overrides: Value) -> Value {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut value = json!({
            "sub":"user-123","iss":"https://issuer.example.test/pool",
            "exp":now+300,"iat":now,"token_use":"access","client_id":"expected-client"
        });
        for (key, val) in overrides.as_object().unwrap() {
            value[key.as_str()] = val.clone();
        }
        value
    }

    fn signed_token(pem: &str, kid: &str, claims: Value) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.into());
        encode(
            &header,
            &claims,
            &EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn accepts_a_valid_signed_access_token() {
        let f = fixture().await;
        let token = signed_token(&f.private_pem, "key-1", claims(json!({})));
        assert_eq!(f.verifier.verify(&token).await.unwrap().subject, "user-123");
        f.server.abort();
    }

    #[tokio::test]
    async fn rejects_bad_signature_and_invalid_claims() {
        let f = fixture().await;
        let valid = signed_token(&f.private_pem, "key-1", claims(json!({})));
        let mut parts = valid.split('.').map(str::to_owned).collect::<Vec<_>>();
        let replacement = if parts[2].starts_with('A') { "B" } else { "A" };
        parts[2].replace_range(..1, replacement);
        assert_eq!(
            f.verifier.verify(&parts.join(".")).await,
            Err(AuthError::InvalidToken)
        );

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let invalid = [
            json!({"iss":"https://wrong.example"}),
            json!({"exp":now-60}),
            json!({"nbf":now+600}),
            json!({"token_use":"id"}),
            json!({"client_id":"wrong-client"}),
            json!({"sub":""}),
        ];
        for overrides in invalid {
            let token = signed_token(&f.private_pem, "key-1", claims(overrides));
            assert_eq!(
                f.verifier.verify(&token).await,
                Err(AuthError::InvalidToken)
            );
        }
        f.server.abort();
    }

    #[tokio::test]
    async fn refreshes_jwks_for_a_rotated_signing_key_id() {
        let f = fixture().await;
        let first = signed_token(&f.private_pem, "key-1", claims(json!({})));
        assert!(f.verifier.verify(&first).await.is_ok());

        {
            let mut cache = f.verifier.cache.write().await;
            cache.last_refresh_at = None;
            cache.last_refresh_failed = false;
        }
        let mut document = f.document.write().await;
        document["keys"][0]["kid"] = json!("key-2");
        drop(document);

        let rotated = signed_token(&f.private_pem, "key-2", claims(json!({})));
        assert_eq!(
            f.verifier.verify(&rotated).await.unwrap().subject,
            "user-123"
        );
        f.server.abort();
    }

    #[tokio::test]
    async fn jwks_outage_fails_closed() {
        let config = CognitoConfig {
            issuer: "https://issuer.example.test/pool".into(),
            app_client_id: "expected-client".into(),
        };
        let verifier = CognitoTokenVerifier::new_with_jwks_url(
            config,
            "http://127.0.0.1:1/.well-known/jwks.json".into(),
        )
        .unwrap();
        let f = fixture().await;
        let token = signed_token(&f.private_pem, "key-1", claims(json!({})));
        assert_eq!(verifier.verify(&token).await, Err(AuthError::Unavailable));
        f.server.abort();
    }
}
