// Thin wrapper around PeerJS for wasm-bindgen
export function newPeer(id) {
    // Use PeerJS cloud server with better error handling
    return new Peer(id, { 
        debug: 2,
        config: {
            iceServers: [
                { urls: 'stun:stun.l.google.com:19302' },
                { urls: 'stun:global.stun.twilio.com:3478' }
            ]
        }
    });
}
