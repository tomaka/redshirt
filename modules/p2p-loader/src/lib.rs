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

use futures::prelude::*;
use libp2p_core::transport::Transport;
use libp2p_core::{identity, muxing::StreamMuxerBox, upgrade};
use libp2p_kad::{
    record::store::{MemoryStore, MemoryStoreConfig},
    record::Key,
    Kademlia, KademliaConfig, KademliaEvent, Quorum,
};
use libp2p_mplex::MplexConfig;
//use libp2p_noise::NoiseConfig;
use libp2p_plaintext::PlainText2Config;
use libp2p_swarm::{Swarm, SwarmEvent};
use std::{collections::VecDeque, io, path::PathBuf, pin::Pin, time::Duration};

mod notifier;

/// Active set of connections to the network.
pub struct Network<T> {
    // TODO: should have identify and ping as well
    swarm: Swarm<Kademlia<MemoryStore>>,

    /// Stream from the files watcher.
    notifications: Pin<Box<dyn Stream<Item = notifier::NotifierEvent>>>,

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
#[non_exhaustive]
pub struct NetworkConfig {
    /// Hardcoded private key, or `None` to generate one automatically.
    pub private_key: Option<[u8; 32]>,

    /// If `Some`, all the files in this directory and children directories will be automatically
    /// pushed onto the DHT.
    ///
    /// If `#[cfg(feature = "notify")]` isn't enabled, passing `Some` will panic at
    /// initialization.
    pub watched_directory: Option<PathBuf>,
}

impl<T> Network<T> {
    /// Initializes the network.
    pub fn start(config: NetworkConfig) -> Result<Network<T>, io::Error> {
        let notifications = if let Some(watched_directory) = config.watched_directory {
            notifier::start_notifier(watched_directory)?.boxed()
        } else {
            stream::pending().boxed()
        };

        let local_keypair = if let Some(mut private_key) = config.private_key {
            let key = identity::ed25519::SecretKey::from_bytes(&mut private_key).unwrap();
            identity::Keypair::Ed25519(From::from(key))
        } else {
            identity::Keypair::generate_ed25519()
        };
        let local_peer_id = local_keypair.public().into_peer_id();
        log::info!("Local peer id: {}", local_peer_id);

        // TODO: libp2p-noise doesn't compile for WASM
        /*let noise_keypair = libp2p_noise::Keypair::new()
        .into_authentic(&local_keypair)
        .unwrap();*/

        let transport = TcpConfig::default()
            .upgrade(upgrade::Version::V1)
            //.authenticate(NoiseConfig::xx(noise_keypair).into_authenticated())
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
            MemoryStore::with_config(
                local_peer_id.clone(),
                MemoryStoreConfig {
                    // TODO: that's a max of 2GB; to increase we should be writing this on disk
                    max_value_bytes: 10 * 1024 * 1024,
                    max_records: 256,
                    ..Default::default()
                },
            ),
            {
                let mut cfg = KademliaConfig::default();
                // TODO: files that are too large don't go through the Kademlia protocol size limit; this should be configured here
                cfg.set_replication_interval(Some(Duration::from_secs(60)));
                cfg
            },
        );

        let mut swarm = Swarm::new(transport, kademlia, local_peer_id);

        // Don't panic if we can't listen on these addresses.
        if let Err(err) = Swarm::listen_on(&mut swarm, "/ip6/::/tcp/30333".parse().unwrap()) {
            log::warn!("Failed to start listener: {}", err);
        }
        if let Err(err) = Swarm::listen_on(&mut swarm, "/ip4/0.0.0.0/tcp/30333".parse().unwrap()) {
            log::warn!("Failed to start listener: {}", err);
        }

        // Bootnode.
        swarm.add_address(
            &"Qmc25MQxSxbUpU49bZ7RVEqgBJPB3SrjG8WVycU3KC7xYP"
                .parse()
                .unwrap(),
            "/ip4/138.68.126.243/tcp/30333".parse().unwrap(),
        );

        swarm.bootstrap();

        Ok(Network {
            swarm,
            notifications,
            active_fetches: Vec::new(),
            events_queue: VecDeque::new(),
        })
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

            let next_event = {
                let from_swarm = self.swarm.next_event();
                let from_notifier = self.notifications.next();
                futures::pin_mut!(from_swarm, from_notifier);
                match future::select(from_swarm, from_notifier).await {
                    future::Either::Left((ev, _)) => future::Either::Left(ev),
                    future::Either::Right((ev, _)) => future::Either::Right(ev),
                }
            };

            match next_event {
                future::Either::Left(SwarmEvent::Behaviour(KademliaEvent::GetRecordResult(
                    Ok(result),
                ))) => {
                    for record in result.records {
                        log::debug!("Successfully loaded record from DHT: {:?}", record.key);
                        if let Some(pos) = self
                            .active_fetches
                            .iter()
                            .position(|(key, _)| *key == record.key)
                        {
                            let user_data = self.active_fetches.remove(pos).1;
                            self.events_queue.push_back(NetworkEvent::FetchSuccess {
                                data: record.value,
                                user_data,
                            });
                        }
                    }
                }
                future::Either::Left(SwarmEvent::Behaviour(KademliaEvent::GetRecordResult(
                    Err(err),
                ))) => {
                    log::info!("Failed to get record: {:?}", err);
                    let fetch_failed_key = err.into_key();
                    if let Some(pos) = self
                        .active_fetches
                        .iter()
                        .position(|(key, _)| *key == fetch_failed_key)
                    {
                        let user_data = self.active_fetches.remove(pos).1;
                        self.events_queue
                            .push_back(NetworkEvent::FetchFail { user_data });
                    }
                }
                future::Either::Left(SwarmEvent::Behaviour(ev)) => {
                    log::info!("Other event: {:?}", ev)
                }
                future::Either::Left(SwarmEvent::Connected(peer)) => {
                    log::trace!("Connected to {:?}", peer)
                }
                future::Either::Left(SwarmEvent::Disconnected(peer)) => {
                    log::trace!("Disconnected from {:?}", peer)
                }
                future::Either::Left(SwarmEvent::NewListenAddr(_)) => {}
                future::Either::Left(SwarmEvent::ExpiredListenAddr(_)) => {}
                future::Either::Left(SwarmEvent::UnreachableAddr { .. }) => {}
                future::Either::Left(SwarmEvent::StartConnect(_)) => {}
                future::Either::Right(Some(notifier::NotifierEvent::InjectDht { hash, data })) => {
                    // TODO: use Quorum::Majority when network is large enough
                    // TODO: is republication automatic?
                    self.swarm.put_record(
                        libp2p_kad::Record::new(hash.to_vec(), data),
                        libp2p_kad::Quorum::One,
                    );
                }
                future::Either::Right(None) => panic!(),
            }
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        NetworkConfig {
            private_key: None,
            watched_directory: None,
        }
    }
}
