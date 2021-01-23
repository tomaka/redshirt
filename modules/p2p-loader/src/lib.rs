// Copyright (C) 2019-2021  Pierre Krieger
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
use libp2p::core::transport::Transport;
use libp2p::core::{identity, muxing::StreamMuxerBox, upgrade};
use libp2p::kad::{
    record::store::{MemoryStore, MemoryStoreConfig},
    record::Key,
    Kademlia, KademliaConfig, KademliaEvent, QueryResult, Quorum,
};
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::yamux;
use std::{collections::VecDeque, io, path::PathBuf, pin::Pin, time::Duration};

mod git_clones;
mod notifier;

/// Active set of connections to the network.
pub struct Network<T> {
    // TODO: should have identify and ping as well
    swarm: Swarm<Kademlia<MemoryStore>>,

    /// Stream from the files watcher.
    notifications: stream::SelectAll<Pin<Box<dyn Stream<Item = notifier::NotifierEvent> + Send>>>,

    /// True if we are connected to any node and have reported it through a
    /// [`NetworkEvent::Readiness`].
    // TODO: never set to false
    connected_to_network: bool,

    /// Holds active git clones.
    _git_clones_directories: git_clones::GitClones,

    /// List of keys that are currently being fetched.
    active_fetches: Vec<(Key, T)>,

    /// Queue of events to return to the user.
    events_queue: VecDeque<NetworkEvent<T>>,
}

/// Event that can happen in a [`Network`].
// TODO: better Debug impl? `data` might be huge
#[derive(Debug)]
pub enum NetworkEvent<T> {
    /// If true, indicates that we're now connected to the peer-to-peer network. If false,
    /// indicates that we're not.
    ///
    /// The [`Network`] starts in a "not ready" state, and this event indicates a switch in
    /// readiness.
    ///
    /// Not being ready has no incidence on how the API is allowed to be used, but queries will
    /// fail unless they hit the local cache.
    // TODO: nothing ever reports false
    Readiness(bool),

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

    /// All the files in this list of directories and children directories will be automatically
    /// pushed onto the DHT.
    ///
    /// If `#[cfg(feature = "notify")]` isn't enabled, passing a non-empty list will panic at
    /// initialization.
    // TODO: what happens if the same path is present multiple times? or if one element is a child
    // of another?
    pub watched_directories: Vec<PathBuf>,

    /// URLs of git repositories whose Wasm files will be automatically pushed to the DHT.
    ///
    /// If `#[cfg(feature = "git")]` isn't enabled, passing a non-empty list will panic at
    /// initialization.
    pub watched_git_repositories: Vec<String>,
}

impl<T> Network<T> {
    /// Initializes the network.
    pub fn start(config: NetworkConfig) -> Result<Network<T>, io::Error> {
        let git_clones_directories = git_clones::clone_git_repos(&config.watched_git_repositories)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let notifications = {
            let mut list = stream::SelectAll::new();
            for directory in config.watched_directories {
                list.push(notifier::start_notifier(directory)?.boxed());
            }
            for path in git_clones_directories.paths() {
                list.push(notifier::start_notifier(path.to_owned())?.boxed());
            }
            // We have to push at least a pending stream, otherwise the `SelectAll` will produce
            // `None`.
            if list.is_empty() {
                list.push(stream::pending().boxed());
            }
            list
        };

        let local_keypair = if let Some(mut private_key) = config.private_key {
            let key = identity::ed25519::SecretKey::from_bytes(&mut private_key).unwrap();
            identity::Keypair::Ed25519(From::from(key))
        } else {
            identity::Keypair::generate_ed25519()
        };
        let local_peer_id = local_keypair.public().into_peer_id();
        log::info!("Local peer id: {}", local_peer_id);

        let noise_keypair = libp2p::noise::Keypair::<libp2p::noise::X25519Spec>::new()
            .into_authentic(&local_keypair)
            .unwrap();

        let transport = TcpConfig::default()
            .upgrade(upgrade::Version::V1)
            .authenticate(libp2p::noise::NoiseConfig::xx(noise_keypair).into_authenticated())
            .multiplex({
                let mut yamux_config = yamux::Config::default();
                // Only set SYN flag on first data frame sent to the remote.
                yamux_config.set_lazy_open(true);
                yamux_config.set_window_update_mode(yamux::WindowUpdateMode::OnRead);
                yamux_config
            })
            // TODO: timeout
            .map(|(id, muxer), _| (id, StreamMuxerBox::new(muxer)))
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
            .boxed();

        let kademlia = Kademlia::with_config(
            local_peer_id.clone(),
            MemoryStore::with_config(
                local_peer_id.clone(),
                MemoryStoreConfig {
                    // TODO: that's a max of 2GB; we should instead be writing this on disk
                    max_value_bytes: 10 * 1024 * 1024,
                    max_records: 256,
                    ..Default::default()
                },
            ),
            {
                let mut cfg = KademliaConfig::default();
                cfg.set_max_packet_size(10 * 1024 * 1024);
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

        // Bootnodes.
        swarm.add_address(
            &"12D3KooWDUiCzY8DqEXeU7gjh5pMjp5WgTjWH7Vnz5SjpwbWHybX"
                .parse()
                .unwrap(),
            "/ip4/157.245.20.120/tcp/30333".parse().unwrap(),
        );
        swarm.add_address(
            &"12D3KooWP8mJmdTPG3mCPRXS9etoTPbYXDniTNKZFfEWHPfFvzKi"
                .parse()
                .unwrap(),
            "/ip4/68.183.243.252/tcp/30333".parse().unwrap(),
        );

        // Bootstrapping returns an error if we don't know of any other peer to connect to.
        // This would normally only happen on the bootnodes themselves.
        let _ = swarm.bootstrap();

        Ok(Network {
            swarm,
            notifications,
            connected_to_network: false,
            _git_clones_directories: git_clones_directories,
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
                future::Either::Left(SwarmEvent::Behaviour(KademliaEvent::QueryResult {
                    result: QueryResult::GetRecord(Ok(result)),
                    ..
                })) => {
                    for record in result.records {
                        log::debug!(
                            "Successfully loaded record from DHT: {:?}",
                            record.record.key
                        );
                        while let Some(pos) = self
                            .active_fetches
                            .iter()
                            .position(|(key, _)| *key == record.record.key)
                        {
                            let user_data = self.active_fetches.remove(pos).1;
                            self.events_queue.push_back(NetworkEvent::FetchSuccess {
                                data: record.record.value.clone(),
                                user_data,
                            });
                        }
                    }
                }
                future::Either::Left(SwarmEvent::Behaviour(KademliaEvent::QueryResult {
                    result: QueryResult::GetRecord(Err(err)),
                    ..
                })) => {
                    log::info!("Failed to get record: {:?}", err);
                    let fetch_failed_key = err.into_key();
                    while let Some(pos) = self
                        .active_fetches
                        .iter()
                        .position(|(key, _)| *key == fetch_failed_key)
                    {
                        let user_data = self.active_fetches.remove(pos).1;
                        self.events_queue
                            .push_back(NetworkEvent::FetchFail { user_data });
                    }
                }
                future::Either::Left(SwarmEvent::Behaviour(KademliaEvent::QueryResult {
                    result: QueryResult::Bootstrap(_),
                    ..
                })) => {}
                future::Either::Left(SwarmEvent::Behaviour(ev)) => {
                    log::info!("Other event: {:?}", ev)
                }
                future::Either::Left(SwarmEvent::ConnectionEstablished { peer_id, .. }) => {
                    log::trace!("Connected to {:?}", peer_id);
                    if !self.connected_to_network {
                        self.connected_to_network = true;
                        self.events_queue.push_back(NetworkEvent::Readiness(true));
                    }
                }
                future::Either::Left(SwarmEvent::ConnectionClosed { peer_id, .. }) => {
                    log::trace!("Disconnected from {:?}", peer_id)
                }
                future::Either::Left(SwarmEvent::NewListenAddr(_)) => {}
                future::Either::Left(SwarmEvent::ExpiredListenAddr(_)) => {}
                future::Either::Left(SwarmEvent::UnreachableAddr { .. }) => {}
                future::Either::Left(SwarmEvent::Dialing(_)) => {}
                future::Either::Left(SwarmEvent::IncomingConnection { .. }) => {}
                future::Either::Left(SwarmEvent::IncomingConnectionError { .. }) => {}
                future::Either::Left(SwarmEvent::BannedPeer { .. }) => {}
                future::Either::Left(SwarmEvent::UnknownPeerUnreachableAddr { .. }) => {}
                future::Either::Left(SwarmEvent::ListenerError { .. }) => {}
                future::Either::Left(SwarmEvent::ListenerClosed { reason, .. }) => {
                    log::warn!("Listener closed: {:?}", reason);
                }
                future::Either::Right(Some(notifier::NotifierEvent::InjectDht { hash, data })) => {
                    // TODO: use Quorum::Majority when network is large enough
                    // This stores the record in the local storage. Republication on the DHT
                    // is then automatically handled by `libp2p-kad`.
                    self.swarm
                        .put_record(
                            libp2p::kad::Record::new(hash.to_vec(), data),
                            libp2p::kad::Quorum::One,
                        )
                        .unwrap();
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
            watched_directories: Vec::new(),
            watched_git_repositories: Vec::new(),
        }
    }
}
