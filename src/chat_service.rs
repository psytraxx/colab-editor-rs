use std::{cell::RefCell, rc::Rc};

use crate::chat::{
    chat_model::{ChatModelMessage, ConnectionString, Message, MessageSender},
    web_rtc_manager::{
        self,
        config::RTCConfig,
        connection_state::{ConnectionState, State},
        IceCandidate,
    },
};
use base64::{self, prelude::BASE64_STANDARD, Engine};
use js_sys::JSON;
use wasm_bindgen::{prelude::Closure, JsCast, JsValue};
use web_sys::{
    console, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelInit, RtcIceConnectionState,
    RtcIceGatheringState, RtcPeerConnection, RtcPeerConnectionIceEvent, RtcSessionDescriptionInit,
};

type SingleArgJsFn = Box<dyn FnMut(JsValue)>;
type SingleArgClosure = Closure<dyn FnMut(JsValue)>;

pub trait ChatService: Clone {
    fn get_state(&self) -> State;
    fn connect_client(&mut self);
    fn disconnect(&mut self);
    fn get_ice_candidates(&self) -> Vec<IceCandidate>;
    fn get_offer(&self) -> Option<String>;
}

#[derive(Debug, PartialEq, Clone)]
pub struct WebRTCChatService {
    state: State,
    ice_candidates: Vec<IceCandidate>,
    offer: Option<String>,
    rtc_peer_connection: Option<RtcPeerConnection>,
    config: RTCConfig,
    data_channel: Option<RtcDataChannel>,
}

impl WebRTCChatService {
    pub fn new(stun_server: &str) -> Self {
        Self {
            config: RTCConfig {
                stun_server: stun_server.to_string(),
            },
            state: State::Default,
            ice_candidates: Vec::new(),
            offer: None,
            rtc_peer_connection: None,
            data_channel: None,
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

    fn start_web_rtc(self) -> Result<(), JsValue> {
        Self::start_web_rtc_impl(Rc::new(RefCell::new(self)))
    }

    fn start_web_rtc_impl(web_rtc_manager: Rc<RefCell<Self>>) -> Result<(), JsValue> {
        let rtc_peer_connection = {
            let config = web_rtc_manager.borrow().config.to_rtc_configuration();
            RtcPeerConnection::new_with_configuration(&config)?
        };

        let create_offer_exception_handler = WebRTCChatService::get_exception_handler(
            web_rtc_manager.clone(),
            "create_offer closure has encountered an exception".into(),
        );

        let state = web_rtc_manager.borrow().state;

        match state {
            State::Server(_connection_state) => {
                let web_rtc_manager_rc_clone = web_rtc_manager.clone();

                let data_channel_init = RtcDataChannelInit::new();
                data_channel_init.set_ordered(true);

                let data_channel: RtcDataChannel = rtc_peer_connection
                    .create_data_channel_with_data_channel_dict("sendChannel", &data_channel_init);

                WebRTCChatService::set_data_channel(web_rtc_manager.clone(), data_channel);

                let create_offer_function: SingleArgJsFn = Box::new(move |offer: JsValue| {
                    let rtc_session_description: RtcSessionDescriptionInit =
                        offer.unchecked_into::<RtcSessionDescriptionInit>();

                    console::log_1(&rtc_session_description.clone().into());

                    web_rtc_manager_rc_clone.borrow_mut().offer = Some(String::from(
                        JSON::stringify(&rtc_session_description).unwrap(),
                    ));

                    let set_local_description_exception_handler =
                        WebRTCChatService::get_exception_handler(
                            web_rtc_manager_rc_clone.clone(),
                            "set_local_description closure has encountered an exception".into(),
                        );

                    let _promise = web_rtc_manager_rc_clone
                        .borrow_mut()
                        .rtc_peer_connection
                        .as_ref()
                        .unwrap()
                        .set_local_description(&rtc_session_description)
                        .catch(&set_local_description_exception_handler);
                });

                let create_offer_closure = Closure::wrap(create_offer_function);

                let _create_offer_promise = rtc_peer_connection
                    .create_offer()
                    .then(&create_offer_closure)
                    .catch(&create_offer_exception_handler);

                create_offer_closure.forget();
            }

            State::Client(_connection_state) => {
                let clone = web_rtc_manager.clone();

                let on_data_channel_closure =
                    Closure::wrap(Box::new(move |data_channel_event: JsValue| {
                        let data_channel_event =
                            data_channel_event.unchecked_into::<RtcDataChannelEvent>();
                        let data_channel = data_channel_event.channel();
                        WebRTCChatService::set_data_channel(clone.clone(), data_channel);
                    }) as SingleArgJsFn);

                rtc_peer_connection
                    .set_ondatachannel(Some(on_data_channel_closure.as_ref().unchecked_ref()));

                on_data_channel_closure.forget();
            }

            _ => {
                panic!("Not implemented");
            }
        };

        let web_rtc_manager_argument = web_rtc_manager.clone();
        let on_ice_candidate_closure =
            Closure::wrap(Box::new(move |ice_connection_event: JsValue| {
                console::log_1(&ice_connection_event);

                let ice_connection_event_obj: RtcPeerConnectionIceEvent =
                    ice_connection_event.unchecked_into::<RtcPeerConnectionIceEvent>();

                if let Some(candidate) = ice_connection_event_obj.candidate() {
                    let candidate_str = candidate.candidate();

                    if !candidate_str.is_empty() {
                        console::log_1(&candidate_str.clone().into());

                        let saved_candidate = IceCandidate {
                            candidate: candidate_str,
                            sdp_mid: candidate.sdp_mid().unwrap(),
                            sdp_m_line_index: candidate.sdp_m_line_index().unwrap(),
                        };

                        web_rtc_manager_argument
                            .borrow_mut()
                            .ice_candidates
                            .push(saved_candidate);
                    }
                }
            }) as SingleArgJsFn);

        let on_ice_connection_state_change_closure =
            WebRTCChatService::get_on_ice_connection_state_change_closure(web_rtc_manager.clone());

        let on_ice_gathering_state_change_closure =
            WebRTCChatService::get_on_ice_gathering_state_change_closure(web_rtc_manager.clone());

        rtc_peer_connection
            .set_onicecandidate(Some(on_ice_candidate_closure.as_ref().unchecked_ref()));

        rtc_peer_connection.set_oniceconnectionstatechange(Some(
            on_ice_connection_state_change_closure
                .as_ref()
                .unchecked_ref(),
        ));

        rtc_peer_connection.set_onicegatheringstatechange(Some(
            on_ice_gathering_state_change_closure
                .as_ref()
                .unchecked_ref(),
        ));

        web_rtc_manager.borrow_mut().rtc_peer_connection = Some(rtc_peer_connection);

        on_ice_candidate_closure.forget();
        on_ice_connection_state_change_closure.forget();
        on_ice_gathering_state_change_closure.forget();

        Ok(())
    }

    fn get_on_ice_connection_state_change_closure(
        web_rtc_manager: Rc<RefCell<WebRTCChatService>>,
    ) -> SingleArgClosure {
        Closure::wrap(Box::new(move |_ice_connection_state_event: JsValue| {
            let ice_new_state: RtcIceConnectionState = {
                let inner = web_rtc_manager.borrow();
                let connection: &RtcPeerConnection = inner.rtc_peer_connection.as_ref().unwrap();
                connection.ice_connection_state()
            };

            let self_state = web_rtc_manager.borrow().get_state();

            let new_state = match self_state {
                State::Server(mut connection_state) => {
                    connection_state.ice_connection_state = Some(ice_new_state);
                    State::Server(connection_state)
                }
                State::Client(mut connection_state) => {
                    connection_state.ice_connection_state = Some(ice_new_state);
                    State::Client(connection_state)
                }
                a => a,
            };

            web_rtc_manager.borrow_mut().set_state(new_state);

            let web_rtc_state = web_rtc_manager.borrow().get_state();

            //TODO: (web_rtc_manager.borrow().message_callback)(ChatModelMessage::UpdateWebRTCState(                web_rtc_state,            ));
        }) as SingleArgJsFn)
    }

    fn get_on_ice_gathering_state_change_closure(
        web_rtc_manager: Rc<RefCell<WebRTCChatService>>,
    ) -> SingleArgClosure {
        Closure::wrap(Box::new(move |_ice_gathering_state: JsValue| {
            let ice_new_state: RtcIceGatheringState = {
                let inner = web_rtc_manager.borrow();
                let connection: &RtcPeerConnection = inner.rtc_peer_connection.as_ref().unwrap();
                connection.ice_gathering_state()
            };

            let self_state = web_rtc_manager.borrow().get_state();

            let new_state = match self_state {
                State::Server(mut connection_state) => {
                    connection_state.ice_gathering_state = Some(ice_new_state);
                    State::Server(connection_state)
                }
                State::Client(mut connection_state) => {
                    connection_state.ice_gathering_state = Some(ice_new_state);
                    State::Client(connection_state)
                }
                a => a,
            };

            web_rtc_manager.borrow_mut().set_state(new_state);
            let web_rtc_state = web_rtc_manager.borrow().get_state();

            //TODO: (web_rtc_manager.borrow().message_callback)(ChatModelMessage::UpdateWebRTCState(                web_rtc_state, ));
        }) as SingleArgJsFn)
    }

    fn set_data_channel(
        web_rtc_manager: Rc<RefCell<WebRTCChatService>>,
        data_channel: RtcDataChannel,
    ) {
        let channel_status_change_closure =
            WebRTCChatService::get_channel_status_change_closure(web_rtc_manager.clone());

        data_channel.set_onopen(Some(channel_status_change_closure.as_ref().unchecked_ref()));
        data_channel.set_onclose(Some(channel_status_change_closure.as_ref().unchecked_ref()));

        channel_status_change_closure.forget();

        let on_data_closure = WebRTCChatService::get_on_data_closure(web_rtc_manager.clone());
        data_channel.set_onmessage(Some(on_data_closure.as_ref().unchecked_ref()));

        on_data_closure.forget();

        web_rtc_manager.borrow_mut().data_channel = Some(data_channel);
    }

    fn get_on_data_closure(web_rtc_manager: Rc<RefCell<WebRTCChatService>>) -> SingleArgClosure {
        Closure::wrap(Box::new(move |arg: JsValue| {
            let message_event = arg.unchecked_into::<web_sys::MessageEvent>();

            let msg_content: String = message_event.data().as_string().unwrap();
            let msg = Message::new(msg_content, MessageSender::Other);

            //TODO: (web_rtc_manager.borrow().message_callback)(ChatModelMessage::NewMessage(msg));
        }) as SingleArgJsFn)
    }

    fn get_channel_status_change_closure(
        web_rtc_manager: Rc<RefCell<WebRTCChatService>>,
    ) -> SingleArgClosure {
        Closure::wrap(Box::new(move |_send_channel: JsValue| {
            let state = web_rtc_manager
                .borrow()
                .data_channel
                .as_ref()
                .unwrap()
                .ready_state();

            let self_state = web_rtc_manager.borrow().get_state();

            let new_state = match self_state {
                State::Server(mut connection_state) => {
                    connection_state.data_channel_state = Some(state);
                    State::Server(connection_state)
                }
                State::Client(mut connection_state) => {
                    connection_state.data_channel_state = Some(state);
                    State::Client(connection_state)
                }
                a => a,
            };

            web_rtc_manager.borrow_mut().set_state(new_state);

            let web_rtc_state = web_rtc_manager.borrow().get_state();

            /* (web_rtc_manager.borrow().message_callback)(ChatModelMessage::UpdateWebRTCState(
                web_rtc_state,
            )); */
        }) as SingleArgJsFn)
    }

    fn set_state(&mut self, new_state: State) {
        self.state = new_state;
    }

    fn get_exception_handler(
        _web_rtc_manager: Rc<RefCell<WebRTCChatService>>,
        message: String,
    ) -> SingleArgClosure {
        Closure::wrap(Box::new(move |a: JsValue| {
            // TODO
            console::log_1(&"Exception handler !".into());
            console::log_1(&JsValue::from_str(&message));
            console::log_1(&a);

            web_sys::Window::alert_with_message(
                &web_sys::window().unwrap(),
                "Promise encountered an exception. See console for details",
            )
            .expect("alert should work");
        }) as SingleArgJsFn)
    }
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
