use web_sys::{RtcDataChannelState, RtcIceConnectionState, RtcIceGatheringState};

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct ConnectionState {
    pub ice_gathering_state: Option<RtcIceGatheringState>,
    pub ice_connection_state: Option<RtcIceConnectionState>,
    pub data_channel_state: Option<RtcDataChannelState>,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum State {
    Default,
    Server(ConnectionState),
    Client(ConnectionState),
}

impl ConnectionState {
    pub fn new() -> Self {
        Self {
            ice_gathering_state: None,
            ice_connection_state: None,
            data_channel_state: None,
        }
    }
}
