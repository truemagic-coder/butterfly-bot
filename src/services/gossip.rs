use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use libp2p::gossipsub::{
    self, AllowAllSubscriptionFilter, IdentTopic, IdentityTransform, MessageAuthenticity,
    ValidationMode,
};
use libp2p::swarm::SwarmEvent;
use libp2p::{identity, noise, tcp, yamux, Multiaddr, PeerId, Swarm, Transport};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, RwLock};

use crate::error::{ButterflyBotError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipMessage {
    pub kind: String,
    pub to: String,
    pub from: String,
    pub message_id: u64,
    pub payload: serde_json::Value,
    pub signature: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SignableGossipMessage {
    pub kind: String,
    pub to: String,
    pub from: String,
    pub message_id: u64,
    pub payload: serde_json::Value,
}

enum GossipCommand {
    Publish(GossipMessage),
    Dial(Multiaddr),
}

fn verify_message(message: &GossipMessage) -> Result<()> {
    if message.signature.trim().is_empty() || message.public_key.trim().is_empty() {
        return Err(ButterflyBotError::Runtime("missing signature".to_string()));
    }
    let signable = SignableGossipMessage {
        kind: message.kind.clone(),
        to: message.to.clone(),
        from: message.from.clone(),
        message_id: message.message_id,
        payload: message.payload.clone(),
    };
    let payload_bytes = serde_json::to_vec(&signable)
        .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;
    let signature = BASE64
        .decode(message.signature.as_bytes())
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    let public_key_bytes = BASE64
        .decode(message.public_key.as_bytes())
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    let public_key = identity::PublicKey::try_decode_protobuf(&public_key_bytes)
        .map_err(|e: identity::DecodingError| ButterflyBotError::Runtime(e.to_string()))?;
    if !public_key.verify(&payload_bytes, &signature) {
        return Err(ButterflyBotError::Runtime("invalid signature".to_string()));
    }
    Ok(())
}

#[derive(Clone)]
pub struct GossipHandle {
    cmd_tx: mpsc::Sender<GossipCommand>,
    event_tx: broadcast::Sender<GossipMessage>,
    pub peer_id: PeerId,
    listen_addrs: Arc<RwLock<Vec<Multiaddr>>>,
    keypair: identity::Keypair,
}

impl GossipHandle {
    pub async fn start(
        listen_addrs: Vec<Multiaddr>,
        bootstrap: Vec<Multiaddr>,
        topic_name: &str,
    ) -> Result<Self> {
        let local_key = identity::Keypair::generate_ed25519();
        let peer_id = PeerId::from(local_key.public());

        let transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true))
            .upgrade(libp2p::core::upgrade::Version::V1Lazy)
            .authenticate(
                noise::Config::new(&local_key)
                    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?,
            )
            .multiplex(yamux::Config::default())
            .boxed();

        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .validation_mode(ValidationMode::Strict)
            .heartbeat_interval(Duration::from_secs(10))
            .build()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let mut behaviour = gossipsub::Behaviour::<IdentityTransform, AllowAllSubscriptionFilter>::new(
            MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let topic = IdentTopic::new(topic_name);
        behaviour
            .subscribe(&topic)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

        let mut swarm = Swarm::new(
            transport,
            behaviour,
            peer_id,
            libp2p::swarm::Config::with_tokio_executor(),
        );

        for addr in listen_addrs {
            swarm
                .listen_on(addr)
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        }

        for addr in bootstrap {
            let _ = swarm.dial(addr);
        }

        let (cmd_tx, mut cmd_rx) = mpsc::channel::<GossipCommand>(64);
        let (event_tx, _) = broadcast::channel::<GossipMessage>(256);
        let event_tx_task = event_tx.clone();
        let topic_task = topic.clone();
        let listen_addrs = Arc::new(RwLock::new(Vec::<Multiaddr>::new()));
        let listen_addrs_task = listen_addrs.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(cmd) = cmd_rx.recv() => {
                        match cmd {
                            GossipCommand::Publish(message) => {
                                if let Ok(data) = serde_json::to_vec(&message) {
                                    let _ = swarm.behaviour_mut().publish(topic_task.clone(), data);
                                }
                            }
                            GossipCommand::Dial(addr) => {
                                let _ = swarm.dial(addr);
                            }
                        }
                    }
                    event = swarm.select_next_some() => {
                        match event {
                            SwarmEvent::Behaviour(gossipsub::Event::Message { message, .. }) => {
                                if let Ok(msg) = serde_json::from_slice::<GossipMessage>(&message.data) {
                                    if verify_message(&msg).is_ok() {
                                        let _ = event_tx_task.send(msg);
                                    }
                                }
                            }
                            SwarmEvent::NewListenAddr { address, .. } => {
                                let mut list: tokio::sync::RwLockWriteGuard<'_, Vec<Multiaddr>> =
                                    listen_addrs_task.write().await;
                                if !list.iter().any(|addr| addr == &address) {
                                    list.push(address);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Ok(Self {
            cmd_tx,
            event_tx,
            peer_id,
            listen_addrs,
            keypair: local_key,
        })
    }

    pub async fn publish(&self, message: GossipMessage) -> Result<()> {
        let signable = SignableGossipMessage {
            kind: message.kind.clone(),
            to: message.to.clone(),
            from: message.from.clone(),
            message_id: message.message_id,
            payload: message.payload.clone(),
        };
        let payload_bytes = serde_json::to_vec(&signable)
            .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;
        let signature = self
            .keypair
            .sign(&payload_bytes)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        let public_key = self
            .keypair
            .to_protobuf_encoding()
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        let signed = GossipMessage {
            signature: BASE64.encode(signature),
            public_key: BASE64.encode(public_key),
            ..message
        };

        self.cmd_tx
            .send(GossipCommand::Publish(signed))
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))
    }

    pub async fn dial(&self, addr: Multiaddr) -> Result<()> {
        self.cmd_tx
            .send(GossipCommand::Dial(addr))
            .await
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<GossipMessage> {
        self.event_tx.subscribe()
    }

    pub async fn listen_addrs(&self) -> Vec<Multiaddr> {
        let list = self.listen_addrs.read().await;
        list.clone()
    }
}
