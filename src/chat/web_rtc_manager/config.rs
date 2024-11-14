use web_sys::RtcConfiguration;

pub struct RTCConfig {
    stun_server: String,
}

impl Default for RTCConfig {
    fn default() -> Self {
        Self {
            stun_server: "stun:stun.l.google.com:19302".to_string(),
        }
    }
}

impl RTCConfig {
    pub fn to_rtc_configuration(&self) -> RtcConfiguration {
        let ice_servers = js_sys::Array::new();
        let server_entry = js_sys::Object::new();

        js_sys::Reflect::set(
            &server_entry,
            &"urls".into(),
            &self.stun_server.clone().into(),
        )
        .expect("Failed to set STUN server");

        ice_servers.push(&server_entry);

        let config = RtcConfiguration::new();
        config.set_ice_servers(&ice_servers);
        config
    }
}
