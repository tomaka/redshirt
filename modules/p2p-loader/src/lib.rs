// Copyright (C) 2019-2020  Pierre Krieger
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

#[cfg(target_arch = "wasm32")] // TODO: not great to have cfg blocks
mod tcp_transport;
#[cfg(not(target_arch = "wasm32"))]
use libp2p_tcp::TcpConfig;
#[cfg(target_arch = "wasm32")]
use tcp_transport::TcpConfig;

use libp2p_core::transport::{boxed::Boxed, Transport};
use libp2p_core::{identity, muxing::StreamMuxerBox, nodes::node::Substream, upgrade, PeerId};
use libp2p_kad::{record::store::MemoryStore, record::Key, Kademlia, KademliaConfig, KademliaEvent, Quorum};
use libp2p_mplex::MplexConfig;
use libp2p_plaintext::PlainText2Config;
use libp2p_swarm::{Swarm, SwarmEvent};
use std::{collections::VecDeque, io, time::Duration};

/// Active set of connections to the network.
pub struct Network<T> {
    // TODO: should have identify and ping as well
    swarm: Swarm<
        Boxed<(PeerId, StreamMuxerBox), io::Error>,
        Kademlia<Substream<StreamMuxerBox>, MemoryStore>,
    >,

    /// List of keys that are currently being fetched.
    active_fetches: Vec<(Key, T)>,

    /// Queue of events to return to the user.
    events_queue: VecDeque<NetworkEvent<T>>,
}

/// Event that can happen in a [`Network`].
// TODO: better Debug impl? `data` might be huge
#[derive(Debug)]
pub enum NetworkEvent<T> {
    /// Successfully fetched a resource.
    FetchSuccess {
        /// Data that matches the hash.
        data: Vec<u8>,
        /// User data that was passed to [`Network::start_fetch`].
        user_data: T,
    },
    /// Failed to fetch a resource, either because it isn't available or we reached the timeout.
    FetchFail {
        /// User data that was passed to [`Network::start_fetch`].
        user_data: T,
    },
}

/// Configuration of a [`Network`].
pub struct NetworkConfig {
    /// Hardcoded private key, or `None` to generate one automatically.
    pub private_key: Option<[u8; 32]>,
}

impl<T> Network<T> {
    /// Initializes the network.
    pub fn start(config: NetworkConfig) -> Network<T> {
        let local_keypair = if let Some(mut private_key) = config.private_key {
            let key = identity::ed25519::SecretKey::from_bytes(&mut private_key).unwrap();
            identity::Keypair::Ed25519(From::from(key))
        } else {
            identity::Keypair::generate_ed25519()
        };
        let local_peer_id = local_keypair.public().into_peer_id();
        log::info!("Local peer id: {}", local_peer_id);

        let transport = TcpConfig::default()
            .upgrade(upgrade::Version::V1)
            // TODO: proper encryption
            .authenticate(PlainText2Config {
                local_public_key: local_keypair.public(),
            })
            .multiplex(MplexConfig::default())
            // TODO: timeout
            .map(|(id, muxer), _| (id, StreamMuxerBox::new(muxer)))
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
            .boxed();

        let kademlia = Kademlia::with_config(
            local_peer_id.clone(),
            MemoryStore::new(local_peer_id.clone()),
            {
                let mut cfg = KademliaConfig::default();
                cfg.set_replication_interval(Some(Duration::from_secs(60)));
                cfg
            }
        );

        let mut swarm = Swarm::new(transport, kademlia, local_peer_id);
        Swarm::listen_on(&mut swarm, "/ip6/::/tcp/30333".parse().unwrap()).unwrap();
        Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/30333".parse().unwrap()).unwrap();

        // Bootnode.
        swarm.add_address(
            &"Qmc25MQxSxbUpU49bZ7RVEqgBJPB3SrjG8WVycU3KC7xYP"
                .parse()
                .unwrap(),
            "/ip4/138.68.126.243/tcp/30333".parse().unwrap(),
        );

        swarm.bootstrap();

        // TODO: use All when network is large enough
        // TODO: temporary for testing
        swarm.put_record(libp2p_kad::Record::new(vec![0; 32], vec![5, 6, 7, 8]), libp2p_kad::Quorum::One);
        swarm.get_record(&From::from(vec![0; 32]), libp2p_kad::Quorum::One);

        Network {
            swarm,
            active_fetches: Vec::new(),
            events_queue: VecDeque::new(),
        }
    }

    /// Starts fetching from the network the value corresponding to the given hash.
    ///
    /// The `user_data` is an opaque value that is passed back when the fetch succeeds or fails.
    pub fn start_fetch(&mut self, hash: &[u8; 32], user_data: T) {
        let key = Key::new(hash);
        self.swarm.get_record(&key, Quorum::One); // TODO: use Majority when network is large enough
        self.active_fetches.push((key, user_data));
    }

    /// Returns a future that returns the next event that happens on the network.
    pub async fn next_event(&mut self) -> NetworkEvent<T> {
        loop {
            if let Some(event) = self.events_queue.pop_front() {
                return event;
            }

            match self.swarm.next_event().await {
                SwarmEvent::Behaviour(KademliaEvent::GetRecordResult(Ok(result))) => {
                    for record in result.records {
                        log::debug!("Successfully loaded record from DHT: {:?}", record.key);
                        if let Some(pos) = self.active_fetches.iter().position(|(key, _)| *key == record.key) {
                            let user_data = self.active_fetches.remove(pos).1;
                            self.events_queue.push_back(NetworkEvent::FetchSuccess {
                                data: record.value,
                                user_data,
                            });
                        }
                    }
                },
                SwarmEvent::Behaviour(KademliaEvent::GetRecordResult(Err(err))) => {
                    log::info!("Failed to get record: {:?}", err);
                    let fetch_failed_key = err.into_key();
                    if let Some(pos) = self.active_fetches.iter().position(|(key, _)| *key == fetch_failed_key) {
                        let user_data = self.active_fetches.remove(pos).1;
                        self.events_queue.push_back(NetworkEvent::FetchFail {
                            user_data,
                        });
                    }
                },
                SwarmEvent::Behaviour(ev) => log::info!("Other event: {:?}", ev),
                SwarmEvent::Connected(peer) => log::trace!("Connected to {:?}", peer),
                SwarmEvent::Disconnected(peer) => log::trace!("Disconnected from {:?}", peer),
                SwarmEvent::NewListenAddr(_) => {}
                SwarmEvent::ExpiredListenAddr(_) => {}
                SwarmEvent::UnreachableAddr { .. } => {}
                SwarmEvent::StartConnect(_) => {}
            }
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        NetworkConfig {
            private_key: None,
        }
    }
}
