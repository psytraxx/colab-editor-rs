use crate::chat::web_rtc_manager::connection_state::State;

#[derive(Debug, PartialEq, Clone)]
pub struct WebRTCChatService {
    stun_server: String,
    state: State,
}

impl WebRTCChatService {
    pub fn new(stun_server: &str) -> Self {
        WebRTCChatService {
            stun_server: stun_server.to_string(),
            state: State::Default,
        }
    }
}

pub trait ChatService {
    fn get_state(&self) -> Result<State, String>;
}

impl ChatService for WebRTCChatService {
    fn get_state(&self) -> Result<State, String> {
        Ok(self.state.clone())
    }
}
