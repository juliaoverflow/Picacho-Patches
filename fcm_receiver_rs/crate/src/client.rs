use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use base64::engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use rand::Rng;
use reqwest::blocking::Client as HttpClient;

use crate::crypto::{create_keys, decode_private_key, encode_private_key, public_key_bytes};
use crate::encryption::decrypt_message;
use crate::error::{Error, Result};
use crate::http_client::{
    self, AndroidAppOptions, FcmInstallRequest, FcmRegisterRequest, GcmRegisterRequest,
};
use crate::messages::{
    create_check_in_request, create_login_request_raw, decode_data_message_stanza,
    decode_login_response, ManualDataMessageStanza,
};
use crate::socket_handler::{Reply, SocketHandler};
use crate::util;

use p256::{PublicKey, SecretKey};

pub struct AndroidApp {
    pub gcm_sender_id: String,
    pub android_package: String,
    pub android_package_cert: String,
}

pub struct FcmClient {
    pub vapid_key: String,
    pub api_key: String,
    pub app_id: String,
    pub project_id: String,
    pub gcm_token: Option<String>,
    pub fcm_token: Option<String>,
    pub android_id: u64,
    pub security_token: u64,
    pub heartbeat_interval: Duration,
    pub on_data_message: Option<Arc<dyn Fn(Vec<u8>) + Send + Sync>>,
    pub on_raw_message: Option<Arc<dyn Fn(ManualDataMessageStanza) + Send + Sync>>,
    /// Hears the persistent ids still to be confirmed by the server (see
    /// `persistent_ids`) whenever they change, to save them for the next
    /// run (`set_persistent_ids`).
    pub on_persistent_ids: Option<Arc<dyn Fn(Vec<String>) + Send + Sync>>,
    pub android_app: Option<AndroidApp>,
    pub ios_bundle_id: Option<String>,
    http_client: HttpClient,
    private_key: Option<SecretKey>,
    public_key: Option<PublicKey>,
    auth_secret: Option<Vec<u8>>,
    /// Persistent ids given with `set_persistent_ids` before listening.
    seeded_persistent_ids: Vec<String>,
    installation_auth_token: Option<String>,
    socket_handler: Option<SocketHandler>,
}

impl FcmClient {
    pub fn new(api_key: String, app_id: String, project_id: String) -> Result<Self> {
        let http_client = http_client::build_default_http_client()?;
        Ok(Self {
            vapid_key: String::new(),
            api_key,
            app_id,
            project_id,
            gcm_token: None,
            fcm_token: None,
            android_id: 0,
            security_token: 0,
            heartbeat_interval: Duration::from_secs(600),
            on_data_message: None,
            on_raw_message: None,
            on_persistent_ids: None,
            android_app: None,
            ios_bundle_id: None,
            http_client,
            private_key: None,
            public_key: None,
            auth_secret: None,
            seeded_persistent_ids: Vec::new(),
            installation_auth_token: None,
            socket_handler: None,
        })
    }

    pub fn with_http_client(
        api_key: String,
        app_id: String,
        project_id: String,
        http_client: HttpClient,
    ) -> Self {
        Self {
            vapid_key: String::new(),
            api_key,
            app_id,
            project_id,
            gcm_token: None,
            fcm_token: None,
            android_id: 0,
            security_token: 0,
            heartbeat_interval: Duration::from_secs(600),
            on_data_message: None,
            on_raw_message: None,
            on_persistent_ids: None,
            android_app: None,
            ios_bundle_id: None,
            http_client,
            private_key: None,
            public_key: None,
            auth_secret: None,
            seeded_persistent_ids: Vec::new(),
            installation_auth_token: None,
            socket_handler: None,
        }
    }

    pub fn create_new_keys(&mut self) -> Result<(String, String)> {
        let (private_key, public_key, auth_secret) = create_keys()?;
        let private_key_der = encode_private_key(&private_key)?;
        let private_key_b64 = BASE64_STANDARD.encode(private_key_der);
        let auth_secret_b64 = BASE64_STANDARD.encode(&auth_secret);

        self.private_key = Some(private_key);
        self.public_key = Some(public_key);
        self.auth_secret = Some(auth_secret);

        Ok((private_key_b64, auth_secret_b64))
    }

    pub fn load_keys(&mut self, private_key_b64: &str, auth_secret_b64: &str) -> Result<()> {
        let private_bytes = BASE64_STANDARD
            .decode(private_key_b64)
            .map_err(|_| Error::InvalidData("invalid private key encoding"))?;
        let private_key = decode_private_key(&private_bytes)?;
        let public_key = private_key.public_key();
        let auth_secret = BASE64_STANDARD
            .decode(auth_secret_b64)
            .map_err(|_| Error::InvalidData("invalid auth secret encoding"))?;

        self.private_key = Some(private_key);
        self.public_key = Some(public_key);
        self.auth_secret = Some(auth_secret);

        Ok(())
    }

    pub fn register(&mut self) -> Result<(String, String, u64, u64)> {
        if self.api_key.is_empty() || self.app_id.is_empty() || self.project_id.is_empty() {
            return Err(Error::InvalidData("missing ApiKey/AppId/ProjectId"));
        }

        if self.android_id == 0 || self.security_token == 0 {
            self.check_in_request_gcm()?;
        }

        if self.private_key.is_none() || self.auth_secret.is_none() {
            return Err(Error::InvalidData("keys not loaded"));
        }

        if self.gcm_token.is_none() && self.android_app.is_none() {
            let token = self.register_request_gcm(None)?;
            self.gcm_token = Some(token);
        }

        let (fid, install_token) = self.install_request()?;
        self.installation_auth_token = Some(install_token);

        if self.android_app.is_some() {
            let token = self.register_request_gcm(Some(fid))?;
            self.fcm_token = Some(token);
        } else {
            let token = self.register_request()?;
            self.fcm_token = Some(token);
        }

        let fcm_token = self
            .fcm_token
            .clone()
            .ok_or_else(|| Error::InvalidData("FCM token not available"))?;
        let gcm_token = self.gcm_token.clone().unwrap_or_default();

        Ok((fcm_token, gcm_token, self.android_id, self.security_token))
    }

    fn check_in_request_gcm(&mut self) -> Result<()> {
        let request = create_check_in_request(self.android_id as i64, Some(self.security_token));
        let response = http_client::send_gcm_check_in_request(&self.http_client, &request)?;
        if let Some(android_id) = response.android_id {
            self.android_id = android_id;
        }
        if let Some(security_token) = response.security_token {
            self.security_token = security_token;
        }
        Ok(())
    }

    fn register_request_gcm(&mut self, fid: Option<String>) -> Result<String> {
        let android_app = self.android_app.as_ref().map(|app| AndroidAppOptions {
            gcm_sender_id: app.gcm_sender_id.as_str(),
            android_package: app.android_package.as_str(),
            android_package_cert: app.android_package_cert.as_str(),
        });

        let request = GcmRegisterRequest {
            android_id: self.android_id,
            security_token: self.security_token,
            app_id: self.app_id.as_str(),
            android_app,
            installation_auth: self.installation_auth_token.as_deref(),
            sender_override: None,
            subtype_override: None,
            delete: false,
            extra_params: fid.into_iter().map(|id| ("X-appid".to_owned(), id)).collect(),
        };

        let token = http_client::send_gcm_register_request(&self.http_client, &request)?;
        if self.android_app.is_none() {
            self.gcm_token = Some(token.clone());
        } else {
            self.fcm_token = Some(token.clone());
        }
        Ok(token)
    }

    fn install_request(&mut self) -> Result<(String, String)> {
        let fid = util::generate_firebase_fid()?;
        let android_app = self.android_app.as_ref().map(|app| AndroidAppOptions {
            gcm_sender_id: app.gcm_sender_id.as_str(),
            android_package: app.android_package.as_str(),
            android_package_cert: app.android_package_cert.as_str(),
        });
        let request = FcmInstallRequest {
            api_key: self.api_key.as_str(),
            project_id: self.project_id.as_str(),
            app_id: self.app_id.as_str(),
            fid: fid.as_str(),
            android_app,
            ios_bundle_id: self.ios_bundle_id.as_deref(),
            firebase_client_header: None,
            firebase_client_log_type: None,
            user_agent: None,
        };
        let response = http_client::send_fcm_install_request(&self.http_client, &request)?;
        Ok((response.fid.unwrap_or(fid), response.auth_token.token))
    }

    fn register_request(&mut self) -> Result<String> {
        let public_key = self
            .public_key
            .as_ref()
            .ok_or_else(|| Error::InvalidData("public key missing"))?;
        let auth_secret = self
            .auth_secret
            .as_ref()
            .ok_or_else(|| Error::InvalidData("auth secret missing"))?;
        let installation_auth = self
            .installation_auth_token
            .as_ref()
            .ok_or_else(|| Error::InvalidData("installation auth missing"))?;
        let gcm_token = self
            .gcm_token
            .as_ref()
            .ok_or_else(|| Error::InvalidData("GCM token missing"))?;

        let mut public_key_b64 = URL_SAFE_NO_PAD.encode(public_key_bytes(public_key));
        public_key_b64 = public_key_b64.replace('=', "");

        let mut auth_secret_b64 = URL_SAFE_NO_PAD.encode(auth_secret);
        auth_secret_b64 = auth_secret_b64.replace('=', "");

        let endpoint = http_client::build_endpoint_url(gcm_token);

        let request = FcmRegisterRequest {
            api_key: self.api_key.as_str(),
            project_id: self.project_id.as_str(),
            installation_auth: installation_auth.as_str(),
            vapid_key: self.vapid_key.as_str(),
            auth_secret: auth_secret_b64.as_str(),
            endpoint: endpoint.as_str(),
            p256dh: public_key_b64.as_str(),
            ios_bundle_id: self.ios_bundle_id.as_deref(),
        };

        let response = http_client::send_fcm_register_request(&self.http_client, &request)?;
        Ok(response.token)
    }

    pub fn start_listening(&mut self) -> Result<()> {
        if self.android_id == 0 || self.security_token == 0 {
            return Err(Error::InvalidData("client's AndroidId and SecurityToken hasn't been set. use FcmClient.register() to generate a new AndroidId and SecurityToken"));
        }

        if self.private_key.is_none() || self.auth_secret.is_none() {
            return Err(Error::InvalidData("client's private key hasn't been set. use FcmClient.load_keys() or FcmClient.create_new_keys()"));
        }

        let acks = acks_for(self.android_id);
        let sent_at_login = {
            let mut acks = acks.lock().unwrap();
            for id in std::mem::take(&mut self.seeded_persistent_ids) {
                acks.add_pending(id);
            }
            acks.pending.iter().cloned().collect::<Vec<_>>()
        };
        let login_request =
            create_login_request_raw(self.android_id, self.security_token, &sent_at_login)?;

        let mut socket_handler = SocketHandler::new();
        socket_handler.set_heartbeat_interval(self.heartbeat_interval);

        let on_data_message = self.on_data_message.clone();
        let on_raw_message = self.on_raw_message.clone();
        let on_persistent_ids = self.on_persistent_ids.clone();
        let auth_secret = self.auth_secret.clone();
        let private_key = self.private_key.clone();

        socket_handler.set_on_message_with_reply(move |tag, data| {
            Self::handle_message(
                tag,
                data,
                &on_data_message,
                &on_raw_message,
                &on_persistent_ids,
                &acks,
                &sent_at_login,
                &auth_secret,
                &private_key,
            )
        });

        socket_handler.connect()?;
        socket_handler.send_login_handshake(&login_request)?;

        self.socket_handler = Some(socket_handler);

        if let Some(handler) = self.socket_handler.as_mut() {
            handler.start_socket_handler()
        } else {
            Err(Error::Other(
                "Socket handler initialization failed".to_string(),
            ))
        }
    }

    /// Handles one stanza from the server, and says what to acknowledge.
    ///
    /// Every message with a persistent id is acknowledged as it arrives (a
    /// stream ack), and kept as pending until a later login names it and
    /// the server confirms that login: a message whose acknowledgement was
    /// lost (the connection dropped first) is acknowledged then instead.
    /// Without acknowledgements, the server delivers every message again at
    /// each login. A message that can't be read is acknowledged too, so it
    /// isn't delivered again and again.
    #[allow(clippy::too_many_arguments)]
    fn handle_message(
        tag: u8,
        data: Vec<u8>,
        on_data_message: &Option<Arc<dyn Fn(Vec<u8>) + Send + Sync>>,
        on_raw_message: &Option<Arc<dyn Fn(ManualDataMessageStanza) + Send + Sync>>,
        on_persistent_ids: &Option<Arc<dyn Fn(Vec<String>) + Send + Sync>>,
        acks: &Arc<Mutex<Acks>>,
        sent_at_login: &[String],
        auth_secret: &Option<Vec<u8>>,
        private_key: &Option<SecretKey>,
    ) -> Result<Reply> {
        use crate::consts::*;

        // Told outside the lock, so the callback may ask the client.
        let tell = |pending: Vec<String>| {
            if let Some(callback) = on_persistent_ids {
                callback(pending);
            }
        };
        match tag {
            K_LOGIN_RESPONSE_TAG => {
                let response = decode_login_response(&data)?;
                if let Some(error) = response.error {
                    return Err(Error::Other(format!(
                        "server refused the login: {} {}",
                        error.code.unwrap_or_default(),
                        error.message.unwrap_or_default()
                    )));
                }
                if sent_at_login.is_empty() {
                    return Ok(Reply::default());
                }
                // The server took the login's persistent ids as received.
                let pending = {
                    let mut guard = acks.lock().unwrap();
                    guard.pending.retain(|id| !sent_at_login.contains(id));
                    guard.pending.iter().cloned().collect()
                };
                tell(pending);
                // And by name, as Chromium does for ids it had saved.
                Ok(Reply {
                    selective_ack: sent_at_login.to_vec(),
                    ..Reply::default()
                })
            }
            K_HEARTBEAT_PING_TAG | K_HEARTBEAT_ACK_TAG | K_IQ_STANZA_TAG => Ok(Reply::default()),
            K_CLOSE_TAG => Err(Error::Other("server returned close tag".to_string())),
            K_DATA_MESSAGE_STANZA_TAG => {
                let message = match decode_data_message_stanza(&data) {
                    Ok(message) => message,
                    Err(e) => {
                        // Acknowledged with the rest of the stream.
                        log::warn!("fcm: dropped a data message that couldn't be read: {e}");
                        return Ok(Reply {
                            stream_ack: true,
                            ..Reply::default()
                        });
                    }
                };
                let reply = Reply {
                    stream_ack: message.persistent_id.is_some()
                        || message.immediate_ack == Some(true),
                    ..Reply::default()
                };

                if let Some(ref persistent_id) = message.persistent_id {
                    let pending = {
                        let mut guard = acks.lock().unwrap();
                        if guard.seen(persistent_id) {
                            // Delivered again: acknowledged again, not handled.
                            return Ok(reply);
                        }
                        guard.add_pending(persistent_id.clone());
                        guard.pending.iter().cloned().collect()
                    };
                    tell(pending);
                }

                if let Err(e) =
                    Self::deliver(message, on_data_message, on_raw_message, auth_secret, private_key)
                {
                    log::warn!("fcm: dropped a data message that couldn't be opened: {e}");
                }
                Ok(reply)
            }
            _ => Ok(Reply::default()),
        }
    }

    /// Hands a data message to its callback: decrypted (web push) or as it
    /// came.
    fn deliver(
        message: ManualDataMessageStanza,
        on_data_message: &Option<Arc<dyn Fn(Vec<u8>) + Send + Sync>>,
        on_raw_message: &Option<Arc<dyn Fn(ManualDataMessageStanza) + Send + Sync>>,
        auth_secret: &Option<Vec<u8>>,
        private_key: &Option<SecretKey>,
    ) -> Result<()> {
        let mut crypto_key_bytes: Option<Vec<u8>> = None;
        let mut encryption_bytes: Option<Vec<u8>> = None;
        let mut is_raw = true;

        for app_data in &message.app_data {
            if app_data.key == "crypto-key" {
                is_raw = false;
                if let Some(dh_part) = app_data
                    .value
                    .split(';')
                    .find(|part| part.starts_with("dh="))
                {
                    let encoded = dh_part
                        .get(3..)
                        .ok_or_else(|| Error::InvalidData("invalid dh parameter"))?;
                    crypto_key_bytes = Some(Self::decode_base64_url(encoded)?);
                }
            } else if app_data.key == "encryption" {
                if let Some(salt_part) = app_data
                    .value
                    .split(';')
                    .find(|part| part.starts_with("salt="))
                {
                    let encoded = salt_part
                        .get(5..)
                        .ok_or_else(|| Error::InvalidData("invalid salt parameter"))?;
                    encryption_bytes = Some(Self::decode_base64_url(encoded)?);
                }
            }
        }

        if !is_raw {
            if let (
                Some(crypto_key),
                Some(encryption),
                Some(auth_secret),
                Some(private_key),
            ) = (
                crypto_key_bytes.as_ref(),
                encryption_bytes.as_ref(),
                auth_secret.as_ref(),
                private_key.as_ref(),
            ) {
                let decrypted = decrypt_message(
                    crypto_key,
                    encryption,
                    message
                        .raw_data
                        .as_deref()
                        .ok_or_else(|| Error::InvalidData("missing raw data payload"))?,
                    auth_secret,
                    private_key,
                )?;

                if let Some(callback) = on_data_message {
                    callback(decrypted);
                }
            }
        } else if let Some(callback) = on_raw_message {
            callback(message);
        }

        Ok(())
    }

    fn decode_base64_url(value: &str) -> Result<Vec<u8>> {
        let mut input = value.to_string();
        while input.len() % 4 != 0 {
            input.push('=');
        }

        URL_SAFE
            .decode(input.as_bytes())
            .map_err(|_| Error::InvalidData("invalid base64 value"))
    }

    pub fn close(&mut self) {
        if let Some(mut socket_handler) = self.socket_handler.take() {
            socket_handler.close();
        }
    }

    pub fn remove_persistent_id(&self, id: &str) {
        let acks = acks_for(self.android_id);
        let mut acks = acks.lock().unwrap();
        acks.pending.retain(|p| p != id);
        acks.recent.retain(|p| p != id);
    }

    /// The persistent ids of messages received but not yet confirmed by
    /// the server: the next login names them, which acknowledges them.
    /// Saving them (see `on_persistent_ids`) and giving them back with
    /// `set_persistent_ids` after a restart acknowledges what the last run
    /// may not have.
    pub fn persistent_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = acks_for(self.android_id)
            .lock()
            .unwrap()
            .pending
            .iter()
            .cloned()
            .collect();
        for id in &self.seeded_persistent_ids {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ids
    }

    /// Persistent ids saved from an earlier run (see `persistent_ids`), to
    /// acknowledge at the next login. Their messages aren't handled again
    /// if delivered.
    pub fn set_persistent_ids(&mut self, ids: Vec<String>) {
        self.seeded_persistent_ids = ids;
    }

    pub fn subscribe_to_topic(&self, topic: &str) -> Result<()> {
        let normalized = topic.strip_prefix("/topics/").unwrap_or(topic);
        http_client::validate_topic_name(normalized)?;

        if self.android_id == 0 || self.security_token == 0 {
            return Err(Error::InvalidData(
                "client's AndroidId and SecurityToken hasn't been set. use FcmClient.register() first",
            ));
        }

        let fcm_token = self
            .fcm_token
            .as_ref()
            .ok_or_else(|| Error::InvalidData("FCM token not available"))?;

        let topic_path = format!("/topics/{normalized}");

        let android_app = self.android_app.as_ref().map(|app| AndroidAppOptions {
            gcm_sender_id: app.gcm_sender_id.as_str(),
            android_package: app.android_package.as_str(),
            android_package_cert: app.android_package_cert.as_str(),
        });

        let extra_params = vec![
            ("X-gcm.topic".to_string(), topic_path.clone()),
            ("X-scope".to_string(), topic_path.clone()),
            ("X-subscription".to_string(), fcm_token.clone()),
            ("X-kid".to_string(), Self::generate_kid()),
        ];

        let request = GcmRegisterRequest {
            android_id: self.android_id,
            security_token: self.security_token,
            app_id: self.app_id.as_str(),
            android_app,
            installation_auth: self.installation_auth_token.as_deref(),
            sender_override: Some(fcm_token.clone()),
            subtype_override: Some(fcm_token.clone()),
            delete: false,
            extra_params,
        };

        http_client::send_gcm_register_request(&self.http_client, &request)?;
        Ok(())
    }

    pub fn unsubscribe_from_topic(&self, topic: &str) -> Result<()> {
        let normalized = topic.strip_prefix("/topics/").unwrap_or(topic);
        http_client::validate_topic_name(normalized)?;

        if self.android_id == 0 || self.security_token == 0 {
            return Err(Error::InvalidData(
                "client's AndroidId and SecurityToken hasn't been set. use FcmClient.register() first",
            ));
        }

        let fcm_token = self
            .fcm_token
            .as_ref()
            .ok_or_else(|| Error::InvalidData("FCM token not available"))?;

        let topic_path = format!("/topics/{normalized}");

        let android_app = self.android_app.as_ref().map(|app| AndroidAppOptions {
            gcm_sender_id: app.gcm_sender_id.as_str(),
            android_package: app.android_package.as_str(),
            android_package_cert: app.android_package_cert.as_str(),
        });

        let extra_params = vec![
            ("X-gcm.topic".to_string(), topic_path.clone()),
            ("X-scope".to_string(), topic_path.clone()),
            ("X-subscription".to_string(), fcm_token.clone()),
            ("X-kid".to_string(), Self::generate_kid()),
            ("X-delete".to_string(), "1".to_string()),
        ];

        let request = GcmRegisterRequest {
            android_id: self.android_id,
            security_token: self.security_token,
            app_id: self.app_id.as_str(),
            android_app,
            installation_auth: self.installation_auth_token.as_deref(),
            sender_override: Some(fcm_token.clone()),
            subtype_override: Some(fcm_token.clone()),
            delete: true,
            extra_params,
        };

        http_client::send_gcm_register_request(&self.http_client, &request)?;
        Ok(())
    }

    fn generate_kid() -> String {
        let mut rng = rand::thread_rng();
        let value: u32 = rng.gen_range(1..1_000_000);
        format!("|ID|{}|", value)
    }
}

/// What a device has received, for acknowledging it: kept for the process
/// by Android ID, so a client made for the next connection (as callers make
/// one each time) carries on where the last one stopped.
#[derive(Default)]
struct Acks {
    /// Received, and not yet confirmed by a login that named them; oldest
    /// first.
    pending: VecDeque<String>,
    /// Handled lately, to drop redeliveries; oldest first.
    recent: VecDeque<String>,
}

impl Acks {
    /// Whether this message was handled already.
    fn seen(&self, id: &str) -> bool {
        self.pending.iter().any(|p| p == id) || self.recent.iter().any(|p| p == id)
    }

    fn add_pending(&mut self, id: String) {
        use crate::consts::{MAX_PENDING_PERSISTENT_IDS, MAX_RECENT_PERSISTENT_IDS};
        if !self.recent.contains(&id) {
            self.recent.push_back(id.clone());
            if self.recent.len() > MAX_RECENT_PERSISTENT_IDS {
                self.recent.pop_front();
            }
        }
        if !self.pending.contains(&id) {
            self.pending.push_back(id);
            if self.pending.len() > MAX_PENDING_PERSISTENT_IDS {
                self.pending.pop_front();
            }
        }
    }
}

fn acks_for(android_id: u64) -> Arc<Mutex<Acks>> {
    static ACKS: OnceLock<Mutex<HashMap<u64, Arc<Mutex<Acks>>>>> = OnceLock::new();
    ACKS.get_or_init(Default::default)
        .lock()
        .unwrap()
        .entry(android_id)
        .or_default()
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::{K_DATA_MESSAGE_STANZA_TAG, K_LOGIN_RESPONSE_TAG};
    use crate::messages::{
        marshal_data_message_stanza, marshal_login_response, ManualAppData, ManualErrorInfo,
        ManualLoginResponse,
    };

    type Heard = Arc<Mutex<Vec<String>>>;

    struct Harness {
        acks: Arc<Mutex<Acks>>,
        delivered: Heard,
        saved: Arc<Mutex<Option<Vec<String>>>>,
        on_raw: Option<Arc<dyn Fn(ManualDataMessageStanza) + Send + Sync>>,
        on_ids: Option<Arc<dyn Fn(Vec<String>) + Send + Sync>>,
    }

    impl Harness {
        fn new() -> Self {
            let delivered: Heard = Arc::default();
            let saved: Arc<Mutex<Option<Vec<String>>>> = Arc::default();
            let (d, s) = (delivered.clone(), saved.clone());
            Self {
                acks: Arc::default(),
                delivered,
                saved,
                on_raw: Some(Arc::new(move |m: ManualDataMessageStanza| {
                    d.lock().unwrap().push(m.app_data[0].value.clone());
                })),
                on_ids: Some(Arc::new(move |ids| *s.lock().unwrap() = Some(ids))),
            }
        }

        fn handle(&self, tag: u8, data: Vec<u8>, sent_at_login: &[String]) -> Result<Reply> {
            FcmClient::handle_message(
                tag,
                data,
                &None,
                &self.on_raw,
                &self.on_ids,
                &self.acks,
                sent_at_login,
                &None,
                &None,
            )
        }

        fn pending(&self) -> Vec<String> {
            self.acks.lock().unwrap().pending.iter().cloned().collect()
        }
    }

    fn message(persistent_id: &str, value: &str) -> Vec<u8> {
        marshal_data_message_stanza(&ManualDataMessageStanza {
            persistent_id: Some(persistent_id.into()),
            app_data: vec![ManualAppData {
                key: "encryptedMessage".into(),
                value: value.into(),
            }],
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn messages_are_acknowledged_and_handled_once() {
        let h = Harness::new();
        let reply = h.handle(K_DATA_MESSAGE_STANZA_TAG, message("0:1", "a"), &[]).unwrap();
        assert!(reply.stream_ack, "a message is acknowledged as it arrives");
        assert_eq!(h.pending(), vec!["0:1"]);
        assert_eq!(h.saved.lock().unwrap().clone(), Some(vec!["0:1".to_string()]));
        // Delivered again: acknowledged again, not handled again.
        let reply = h.handle(K_DATA_MESSAGE_STANZA_TAG, message("0:1", "a"), &[]).unwrap();
        assert!(reply.stream_ack);
        assert_eq!(*h.delivered.lock().unwrap(), vec!["a"]);
    }

    #[test]
    fn a_confirmed_login_settles_the_ids_it_named() {
        let h = Harness::new();
        h.handle(K_DATA_MESSAGE_STANZA_TAG, message("0:1", "a"), &[]).unwrap();
        h.handle(K_DATA_MESSAGE_STANZA_TAG, message("0:2", "b"), &[]).unwrap();
        // The next login named 0:1 only (0:2 came after it).
        let sent = vec!["0:1".to_string()];
        let response = marshal_login_response(&ManualLoginResponse::default()).unwrap();
        let reply = h.handle(K_LOGIN_RESPONSE_TAG, response, &sent).unwrap();
        assert_eq!(reply.selective_ack, sent);
        assert_eq!(h.pending(), vec!["0:2"]);
        assert_eq!(h.saved.lock().unwrap().clone(), Some(vec!["0:2".to_string()]));
        // Settled, it is still known: a late redelivery isn't handled.
        h.handle(K_DATA_MESSAGE_STANZA_TAG, message("0:1", "a"), &[]).unwrap();
        assert_eq!(*h.delivered.lock().unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn a_refused_login_keeps_the_ids() {
        let h = Harness::new();
        h.handle(K_DATA_MESSAGE_STANZA_TAG, message("0:1", "a"), &[]).unwrap();
        let response = marshal_login_response(&ManualLoginResponse {
            error: Some(ManualErrorInfo {
                code: Some(401),
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
        assert!(h.handle(K_LOGIN_RESPONSE_TAG, response, &["0:1".into()]).is_err());
        assert_eq!(h.pending(), vec!["0:1"]);
    }

    #[test]
    fn unreadable_messages_are_acknowledged_rather_than_ending_the_connection() {
        let h = Harness::new();
        // Not a data message stanza at all.
        let reply = h.handle(K_DATA_MESSAGE_STANZA_TAG, vec![0xff, 0xff, 0xff], &[]).unwrap();
        assert!(reply.stream_ack);
        // Encrypted, but the client has no keys to open it with.
        let encrypted = marshal_data_message_stanza(&ManualDataMessageStanza {
            persistent_id: Some("0:3".into()),
            app_data: vec![
                ManualAppData {
                    key: "crypto-key".into(),
                    value: "dh=AAAA".into(),
                },
                ManualAppData {
                    key: "encryption".into(),
                    value: "salt=%%%".into(),
                },
            ],
            ..Default::default()
        })
        .unwrap();
        let reply = h.handle(K_DATA_MESSAGE_STANZA_TAG, encrypted, &[]).unwrap();
        assert!(reply.stream_ack);
        assert_eq!(h.pending(), vec!["0:3"]);
        assert!(h.delivered.lock().unwrap().is_empty());
    }

    #[test]
    fn ids_carry_over_to_the_next_client_of_the_device() {
        let android_id = 0x5eed_0001;
        let mut first = FcmClient::with_http_client(
            "key".into(),
            "app".into(),
            "project".into(),
            HttpClient::new(),
        );
        first.android_id = android_id;
        acks_for(android_id).lock().unwrap().add_pending("0:9".into());
        let mut second =
            FcmClient::with_http_client("key".into(), "app".into(), "project".into(), HttpClient::new());
        second.android_id = android_id;
        second.set_persistent_ids(vec!["0:8".into()]);
        let ids = second.persistent_ids();
        assert!(ids.contains(&"0:9".to_string()) && ids.contains(&"0:8".to_string()));
        first.remove_persistent_id("0:9");
        assert_eq!(second.persistent_ids(), vec!["0:8".to_string()]);
    }
}
