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
use libp2p_kad::{record::store::MemoryStore, record::Key, Kademlia, KademliaConfig, Quorum};
use libp2p_mplex::MplexConfig;
use libp2p_plaintext::PlainText2Config;
use libp2p_swarm::{Swarm, SwarmEvent};
use std::{io, time::Duration};

/// Active set of connections to the network.
pub struct Network<T> {
    // TODO: should have identify and ping as well
    swarm: Swarm<
        Boxed<(PeerId, StreamMuxerBox), io::Error>,
        Kademlia<Substream<StreamMuxerBox>, MemoryStore>,
    >,
    active_fetches: Vec<([u8; 32], T)>,
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

impl<T> Network<T> {
    /// Initializes the network.
    pub fn start() -> Network<T> {
        let local_keypair = identity::Keypair::generate_ed25519();
        let local_peer_id = local_keypair.public().into_peer_id();

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
            &"QmfR3LRERsUu6LeEX3XqhykWGqY7Mj49u4yQoMiXuH8ijm" // TODO: wrong; changes at each restart
                .parse()
                .unwrap(),
            "/ip4/138.68.126.243/tcp/30333".parse().unwrap(),
        );

        swarm.bootstrap();

        swarm.put_record(libp2p_kad::Record::new(vec![0; 32], vec![5, 6, 7, 8]), libp2p_kad::Quorum::Majority);

        Network {
            swarm,
            active_fetches: Vec::new(),
        }
    }

    /// Starts fetching from the network the value corresponding to the given hash.
    ///
    /// The `user_data` is an opaque value that is passed back when the fetch succeeds or fails.
    pub fn start_fetch(&mut self, hash: &[u8; 32], user_data: T) {
        self.swarm.get_record(&Key::new(hash), Quorum::One);
        self.active_fetches.push((*hash, user_data));
    }

    /// Returns a future that returns the next event that happens on the network.
    pub async fn next_event(&mut self) -> NetworkEvent<T> {
        loop {
            match self.swarm.next_event().await {
                SwarmEvent::Behaviour(ev) => log::info!("{:?}", ev),
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
