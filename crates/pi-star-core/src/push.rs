use std::path::PathBuf;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes128Gcm, Nonce};
use anyhow::{Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hkdf::Hkdf;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use parking_lot::Mutex;
use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushSubscription {
    pub endpoint: String,
    pub keys: PushKeys,
    #[serde(default = "default_locale")]
    pub locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushKeys {
    pub p256dh: String,
    pub auth: String,
}

fn default_locale() -> String {
    "en".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VapidKeys {
    #[serde(rename = "publicKey")]
    public_key: String,
    #[serde(rename = "privateKey")]
    private_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PushState {
    #[serde(default)]
    vapid_keys: Option<VapidKeys>,
    #[serde(default)]
    subscriptions: Vec<PushSubscription>,
}

impl Default for PushState {
    fn default() -> Self {
        Self {
            vapid_keys: None,
            subscriptions: Vec::new(),
        }
    }
}

pub struct PushStore {
    path: PathBuf,
    state: Mutex<PushState>,
    client: reqwest::Client,
}

impl PushStore {
    pub fn load(path: PathBuf) -> Self {
        let state = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<PushState>(&text).ok())
            .unwrap_or_default();
        Self {
            path,
            state: Mutex::new(state),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn public_key(&self) -> Result<String> {
        let mut state = self.state.lock();
        let keys = self.ensure_keys(&mut state)?;
        Ok(keys.public_key)
    }

    pub fn add_subscription(&self, subscription: PushSubscription) -> Result<()> {
        let mut state = self.state.lock();
        self.ensure_keys(&mut state)?;
        state
            .subscriptions
            .retain(|existing| existing.endpoint != subscription.endpoint);
        state.subscriptions.push(subscription);
        self.save(&state)
    }

    pub async fn notify_session_complete(&self, session_id: &str, session_name: Option<&str>) {
        let (keys, subscriptions) = {
            let state = self.state.lock();
            let Some(keys) = state.vapid_keys.clone() else {
                return;
            };
            (keys, state.subscriptions.clone())
        };
        if subscriptions.is_empty() {
            return;
        }
        let mut prune = Vec::new();
        for subscription in subscriptions {
            let (title, body) = if subscription.locale == "zh-CN" {
                (
                    session_name.unwrap_or("会话完成").to_string(),
                    "任务已完成。".to_string(),
                )
            } else {
                (
                    session_name.unwrap_or("Session complete").to_string(),
                    "Task finished.".to_string(),
                )
            };
            let payload = json!({
                "title": title,
                "body": body,
                "url": format!("/?session={}", urlencoding(session_id)),
                "tag": format!("pi-session-complete:{session_id}"),
            })
            .to_string();
            match self
                .send_one(&subscription, payload.as_bytes(), &keys)
                .await
            {
                Ok(()) => {}
                Err(Some(status)) if status == 404 || status == 410 => {
                    prune.push(subscription.endpoint.clone());
                }
                Err(_) => {}
            }
        }
        if !prune.is_empty() {
            let mut state = self.state.lock();
            state
                .subscriptions
                .retain(|subscription| !prune.contains(&subscription.endpoint));
            let _ = self.save(&state);
        }
    }

    fn ensure_keys(&self, state: &mut PushState) -> Result<VapidKeys> {
        if let Some(keys) = &state.vapid_keys {
            if !keys.public_key.is_empty() && !keys.private_key.is_empty() {
                return Ok(keys.clone());
            }
        }
        let secret = SecretKey::random(&mut OsRng);
        let public = secret.public_key().to_encoded_point(false);
        let keys = VapidKeys {
            public_key: URL_SAFE_NO_PAD.encode(public.as_bytes()),
            private_key: URL_SAFE_NO_PAD.encode(secret.to_bytes()),
        };
        state.vapid_keys = Some(keys.clone());
        self.save(state)?;
        Ok(keys)
    }

    fn save(&self, state: &PushState) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec(state)?;
        let temp = self.path.with_file_name(format!(
            ".{}-{}.tmp",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("web-push"),
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&temp, bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&temp, &self.path)?;
        Ok(())
    }

    async fn send_one(
        &self,
        subscription: &PushSubscription,
        payload: &[u8],
        keys: &VapidKeys,
    ) -> Result<(), Option<u16>> {
        let endpoint = reqwest::Url::parse(&subscription.endpoint).map_err(|_| None::<u16>)?;
        let mut audience = format!(
            "{}://{}",
            endpoint.scheme(),
            endpoint.host_str().ok_or(None::<u16>)?
        );
        if let Some(port) = endpoint.port() {
            audience.push_str(&format!(":{port}"));
        }
        let subject = std::env::var("PI_WEB_PUSH_SUBJECT")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "https://github.com/zeaphoo/pi-star".into());
        let private = URL_SAFE_NO_PAD
            .decode(&keys.private_key)
            .map_err(|_| None::<u16>)?;
        let signing_key = SigningKey::from_bytes((&private[..]).into()).map_err(|_| None::<u16>)?;
        let header = json!({"typ":"JWT","alg":"ES256"});
        let claims = json!({
            "aud": audience,
            "exp": chrono::Utc::now().timestamp() + 12 * 3600,
            "sub": subject,
        });
        let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).map_err(|_| None::<u16>)?);
        let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).map_err(|_| None::<u16>)?);
        let signing_input = format!("{header}.{claims}");
        let signature: Signature = signing_key.sign(signing_input.as_bytes());
        let jwt = format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        );
        let body = encrypt_payload(subscription, payload).map_err(|_| None::<u16>)?;
        let response = self
            .client
            .post(endpoint)
            .header("TTL", "2419200")
            .header("Urgency", "high")
            .header("Content-Encoding", "aes128gcm")
            .header("Content-Type", "application/octet-stream")
            .header(
                "Authorization",
                format!("vapid t={jwt}, k={}", keys.public_key),
            )
            .body(body)
            .send()
            .await
            .map_err(|_| None::<u16>)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(Some(response.status().as_u16()))
        }
    }
}

fn encrypt_payload(subscription: &PushSubscription, payload: &[u8]) -> Result<Vec<u8>> {
    let ua_public = URL_SAFE_NO_PAD
        .decode(&subscription.keys.p256dh)
        .map_err(|_| anyhow!("invalid p256dh"))?;
    let auth = URL_SAFE_NO_PAD
        .decode(&subscription.keys.auth)
        .map_err(|_| anyhow!("invalid auth"))?;
    let ua_public =
        PublicKey::from_sec1_bytes(&ua_public).map_err(|_| anyhow!("invalid p256dh"))?;
    let ephemeral = p256::ecdh::EphemeralSecret::random(&mut OsRng);
    let as_public = ephemeral.public_key().to_encoded_point(false);
    let shared = ephemeral.diffie_hellman(&ua_public);

    let hkdf = Hkdf::<Sha256>::new(Some(&auth), shared.raw_secret_bytes().as_slice());
    let mut ikm = [0u8; 32];
    let mut info = Vec::new();
    info.extend_from_slice(b"WebPush: info\0");
    info.extend_from_slice(ua_public.to_encoded_point(false).as_bytes());
    info.extend_from_slice(as_public.as_bytes());
    hkdf.expand(&info, &mut ikm).map_err(|_| anyhow!("hkdf"))?;

    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let hkdf = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut cek = [0u8; 16];
    let mut nonce = [0u8; 12];
    hkdf.expand(b"Content-Encoding: aes128gcm\0", &mut cek)
        .map_err(|_| anyhow!("hkdf cek"))?;
    hkdf.expand(b"Content-Encoding: nonce\0", &mut nonce)
        .map_err(|_| anyhow!("hkdf nonce"))?;

    // Web Push payload: one record, padded with a 0x02 delimiter.
    let mut plaintext = Vec::with_capacity(payload.len() + 1);
    plaintext.extend_from_slice(payload);
    plaintext.push(2);

    let cipher = Aes128Gcm::new_from_slice(&cek).map_err(|_| anyhow!("aes key"))?;
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &plaintext,
                aad: &[],
            },
        )
        .map_err(|_| anyhow!("aes encrypt"))?;

    let mut body = Vec::with_capacity(16 + 4 + 1 + 65 + ciphertext.len());
    body.extend_from_slice(&salt);
    body.extend_from_slice(&4096u32.to_be_bytes());
    body.push(65);
    body.extend_from_slice(as_public.as_bytes());
    body.extend_from_slice(&ciphertext);
    Ok(body)
}

fn urlencoding(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

pub fn parse_subscription(value: &Value) -> Option<PushSubscription> {
    let endpoint = value.get("endpoint")?.as_str()?.to_string();
    let keys = value.get("keys")?;
    Some(PushSubscription {
        endpoint,
        keys: PushKeys {
            p256dh: keys.get("p256dh")?.as_str()?.to_string(),
            auth: keys.get("auth")?.as_str()?.to_string(),
        },
        locale: value
            .get("locale")
            .and_then(Value::as_str)
            .filter(|locale| *locale == "zh-CN")
            .unwrap_or("en")
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_web_push_compatible_keys() {
        let temp = tempfile::tempdir().unwrap();
        let store = PushStore::load(temp.path().join("web-push.json"));
        let public = store.public_key().unwrap();
        let public = URL_SAFE_NO_PAD.decode(public).unwrap();
        assert_eq!(public.len(), 65);
        assert_eq!(public[0], 4);
        let state = store.state.lock();
        let private = URL_SAFE_NO_PAD
            .decode(&state.vapid_keys.as_ref().unwrap().private_key)
            .unwrap();
        assert_eq!(private.len(), 32);
    }

    #[test]
    fn encrypts_a_payload_record() {
        let subscription = PushSubscription {
            endpoint: "https://example.com/push".into(),
            keys: PushKeys {
                p256dh: "BNDyYhDuDZJ1unUIFxBAM4UggPNXfTQ10VPlJdJ8Vx9psbztEuRj8L_iTuQr3pXbVhct3UVHo_yX1kmvZdpuCzE".into(),
                auth: "46yScNguYqUuUyzJTB_CCw".into(),
            },
            locale: "en".into(),
        };
        let body = encrypt_payload(&subscription, b"{\"title\":\"hi\"}").unwrap();
        assert!(body.len() > 16 + 4 + 1 + 65);
        assert_eq!(body[20], 65);
        assert_eq!(&body[16..20], &4096u32.to_be_bytes());
    }
}
