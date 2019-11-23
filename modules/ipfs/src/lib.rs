// Copyright (C) 2019  Pierre Krieger
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

mod tcp_transport;

use futures::prelude::*;
use libp2p_core::{identity, upgrade, PeerId, muxing::StreamMuxerBox, nodes::node::Substream};
use libp2p_core::transport::{Transport, boxed::Boxed};
use libp2p_kad::{Kademlia, Quorum, record::Key, record::store::MemoryStore};
use libp2p_mplex::MplexConfig;
use libp2p_plaintext::PlainText2Config;
use libp2p_swarm::{Swarm, NetworkBehaviour};
use std::io;

/// Active set of connections to the network.
pub struct Network<T> {
    swarm: Swarm<Boxed<(PeerId, StreamMuxerBox), io::Error>, Kademlia<Substream<StreamMuxerBox>, MemoryStore>>,
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

        let transport = tcp_transport::TcpConfig::default()
            .upgrade(upgrade::Version::V1)
            .authenticate(PlainText2Config {
                local_public_key: local_keypair.public(),
            })
            .multiplex(MplexConfig::default())
            // TODO: timeout
            .map(|(id, muxer), _| (id, StreamMuxerBox::new(muxer)))
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
            .boxed();

        let kademlia = Kademlia::new(local_peer_id.clone(), MemoryStore::new(local_peer_id.clone()));

        let mut swarm = Swarm::new(transport, kademlia, local_peer_id);
        swarm.bootstrap();

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
            self.swarm.next().await;
        }
        // TODO: unfinished

        /*if !self.active_fetches.is_empty() {
            let (_, user_data) = self.active_fetches.remove(0);
            return NetworkEvent::FetchFail { user_data };
        }

        loop {
            futures::pending!()
        }*/
    }
}
