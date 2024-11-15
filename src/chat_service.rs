use crate::chat::{
    chat_model::ConnectionString,
    web_rtc_manager::{
        connection_state::{ConnectionState, State},
        IceCandidate,
    },
};
use base64::{self, prelude::BASE64_STANDARD, Engine};

#[derive(Debug, PartialEq, Clone)]
pub struct WebRTCChatService {
    stun_server: String,
    state: State,
    ice_candidates: Vec<IceCandidate>,
    offer: Option<String>,
}

impl WebRTCChatService {
    pub fn new(stun_server: &str) -> Self {
        Self {
            stun_server: stun_server.to_string(),
            state: State::Default,
            ice_candidates: Vec::new(),
            offer: None,
        }
    }

    fn get_serialized_offer_and_candidates(&self) -> String {
        let connection_string: ConnectionString = ConnectionString {
            offer: self.get_offer().expect("no offer yet"),
            ice_candidates: self.get_ice_candidates(),
        };

        let serialized: String = serde_json::to_string(&connection_string).unwrap();
        BASE64_STANDARD.encode(serialized)
    }
}

pub trait ChatService: Clone {
    fn get_state(&self) -> State;
    fn connect_client(&mut self);
    fn disconnect(&mut self);
    fn get_ice_candidates(&self) -> Vec<IceCandidate>;
    fn get_offer(&self) -> Option<String>;
}

impl ChatService for WebRTCChatService {
    fn get_state(&self) -> State {
        self.state
    }

    fn connect_client(&mut self) {
        self.state = State::Client(ConnectionState::new());
    }

    fn get_ice_candidates(&self) -> Vec<IceCandidate> {
        self.ice_candidates.clone()
    }

    fn get_offer(&self) -> Option<String> {
        self.offer.clone()
    }

    fn disconnect(&mut self) {
        self.state = State::Default;
    }
}
