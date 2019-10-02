// Copyright(c) 2019 Pierre Krieger

use futures::prelude::*;
use parity_scale_codec::DecodeAll;

fn main() {
    syscalls::block_on(async move {
        interface::register_interface(loader::ffi::INTERFACE).await.unwrap();

        loop {
            let msg = syscalls::next_interface_message().await;
            assert_eq!(msg.interface, loader::ffi::INTERFACE);
            let msg_data = loader::ffi::LoaderMessage::decode_all(&msg.actual_data).unwrap();
            let loader::ffi::LoaderMessage::Load(hash_to_load) = msg_data;
            println!("received message: {:?}", hash_to_load);
            syscalls::emit_answer(msg.message_id.unwrap(), &loader::ffi::LoadResponse {
                result: Ok(vec![1, 2, 3, 4])
            });
        }

        /*let mut tcp_stream = tcp::TcpStream::connect(&"127.0.0.1:8000".parse().unwrap()).await;

        tcp_stream
            .write_all(
                br#"GET / HTTP/1.1
User-Agent: Mozilla/4.0 (compatible; MSIE5.01; Windows NT)
Host: localhost
Connection: Keep-Alive

"#,
            )
            .await
            .unwrap();
        tcp_stream.flush().await.unwrap();

        let mut out = vec![0; 65536];
        let out_len = tcp_stream.read(&mut out).await.unwrap();
        out.truncate(out_len);
        println!("out = {:?}", out);*/
    });
}

// TODO: compiles but fails to link because of wasm-in-browser stuff getting in the way
/*use futures01::prelude::*;
use futures::compat::Compat01As03;
use libp2p_core::{
    upgrade::Version,
    upgrade::InboundUpgradeExt,
    upgrade::OutboundUpgradeExt,
    PeerId,
    Transport,
    identity,
};
use libp2p_swarm::Swarm;
use libp2p_kad::{Kademlia, KademliaConfig, KademliaEvent, GetClosestPeersError};
use libp2p_kad::record::store::MemoryStore;
use std::env;
use std::time::Duration;

mod tcp_transport;

fn main() {
    //env_logger::init();

    // Create a random key for ourselves.
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    // Set up a an encrypted DNS-enabled TCP Transport over the Mplex protocol
    let transport = tcp_transport::TcpConfig::new();

    let plaintext_config = libp2p_plaintext::PlainText2Config {
        pubkey: local_key.public(),
    };
    let transport = transport.and_then(move |stream, endpoint| {
        libp2p_core::upgrade::apply(stream, plaintext_config, endpoint, Version::V1)
            .and_then(|(remote_key, stream)| Ok((stream, remote_key)))
    });
    let mut mplex_config = libp2p_mplex::MplexConfig::new();
    let transport = transport.and_then(move |(stream, peer_id), endpoint| {
        let peer_id2 = peer_id.clone();
        let upgrade = mplex_config
            .map_inbound(move |muxer| (peer_id, muxer))
            .map_outbound(move |muxer| (peer_id2, muxer));

        libp2p_core::upgrade::apply(stream, upgrade, endpoint, Version::V1)
            .map(|(id, muxer)| (id, libp2p_core::muxing::StreamMuxerBox::new(muxer)))
    });

    // Create a swarm to manage peers and events.
    let mut swarm = {
        // Create a Kademlia behaviour.
        // Note that normally the Kademlia process starts by performing lots of request in order
        // to insert our local node in the DHT. However here we use `without_init` because this
        // example is very ephemeral and we don't want to pollute the DHT. In a real world
        // application, you want to use `new` instead.
        let mut cfg = KademliaConfig::default();
        cfg.set_query_timeout(Duration::from_secs(5 * 60));
        let store = MemoryStore::new(local_peer_id.clone());
        let mut behaviour = Kademlia::with_config(local_peer_id.clone(), store, cfg);

        // TODO: the /dnsaddr/ scheme is not supported (https://github.com/libp2p/rust-libp2p/issues/967)
        /*behaviour.add_address(&"QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN".parse().unwrap(), "/dnsaddr/bootstrap.libp2p.io".parse().unwrap());
        behaviour.add_address(&"QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa".parse().unwrap(), "/dnsaddr/bootstrap.libp2p.io".parse().unwrap());
        behaviour.add_address(&"QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb".parse().unwrap(), "/dnsaddr/bootstrap.libp2p.io".parse().unwrap());
        behaviour.add_address(&"QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt".parse().unwrap(), "/dnsaddr/bootstrap.libp2p.io".parse().unwrap());*/

        // The only address that currently works.
        behaviour.add_address(&"QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ".parse().unwrap(), "/ip4/104.131.131.82/tcp/4001".parse().unwrap());

        // The following addresses always fail signature verification, possibly due to
        // RSA keys with < 2048 bits.
        // behaviour.add_address(&"QmSoLPppuBtQSGwKDZT2M73ULpjvfd3aZ6ha4oFGL1KrGM".parse().unwrap(), "/ip4/104.236.179.241/tcp/4001".parse().unwrap());
        // behaviour.add_address(&"QmSoLSafTMBsPKadTEgaXctDQVcqN88CNLHXMkTNwMKPnu".parse().unwrap(), "/ip4/128.199.219.111/tcp/4001".parse().unwrap());
        // behaviour.add_address(&"QmSoLV4Bbm51jM9C4gDYZQ9Cy3U6aXMJDAbzgu2fzaDs64".parse().unwrap(), "/ip4/104.236.76.40/tcp/4001".parse().unwrap());
        // behaviour.add_address(&"QmSoLer265NRgSp2LA3dPaeykiS1J6DifTC88f5uVQKNAd".parse().unwrap(), "/ip4/178.62.158.247/tcp/4001".parse().unwrap());

        // The following addresses are permanently unreachable:
        // Other(Other(A(Transport(A(Underlying(Os { code: 101, kind: Other, message: "Network is unreachable" }))))))
        // behaviour.add_address(&"QmSoLPppuBtQSGwKDZT2M73ULpjvfd3aZ6ha4oFGL1KrGM".parse().unwrap(), "/ip6/2604:a880:1:20::203:d001/tcp/4001".parse().unwrap());
        // behaviour.add_address(&"QmSoLSafTMBsPKadTEgaXctDQVcqN88CNLHXMkTNwMKPnu".parse().unwrap(), "/ip6/2400:6180:0:d0::151:6001/tcp/4001".parse().unwrap());
        // behaviour.add_address(&"QmSoLV4Bbm51jM9C4gDYZQ9Cy3U6aXMJDAbzgu2fzaDs64".parse().unwrap(), "/ip6/2604:a880:800:10::4a:5001/tcp/4001".parse().unwrap());
        // behaviour.add_address(&"QmSoLer265NRgSp2LA3dPaeykiS1J6DifTC88f5uVQKNAd".parse().unwrap(), "/ip6/2a03:b0c0:0:1010::23:1001/tcp/4001".parse().unwrap());
        Swarm::new(transport, behaviour, local_peer_id)
    };

    // Order Kademlia to search for a peer.
    let to_search: PeerId = if let Some(peer_id) = env::args().nth(1) {
        peer_id.parse().expect("Failed to parse peer ID to find")
    } else {
        identity::Keypair::generate_ed25519().public().into()
    };

    println!("Searching for the closest peers to {:?}", to_search);
    swarm.get_closest_peers(to_search);

    // Kick it off!
    syscalls::block_on(Compat01As03::new(futures01::future::poll_fn(move || -> Result<_, ()> {
        loop {
            match swarm.poll().expect("Error while polling swarm") {
                Async::Ready(Some(KademliaEvent::GetClosestPeersResult(res))) => {
                    match res {
                        Ok(ok) => {
                            if !ok.peers.is_empty() {
                                println!("Query finished with closest peers: {:#?}", ok.peers);
                                return Ok(Async::Ready(()));
                            } else {
                                // The example is considered failed as there
                                // should always be at least 1 reachable peer.
                                panic!("Query finished with no closest peers.");
                            }
                        }
                        Err(GetClosestPeersError::Timeout { peers, .. }) => {
                            if !peers.is_empty() {
                                println!("Query timed out with closest peers: {:#?}", peers);
                                return Ok(Async::Ready(()));
                            } else {
                                // The example is considered failed as there
                                // should always be at least 1 reachable peer.
                                panic!("Query timed out with no closest peers.");
                            }
                        }
                    }
                },
                Async::Ready(Some(_)) => {},
                Async::Ready(None) | Async::NotReady => break,
            }
        }

        Ok(Async::NotReady)
    })));
}*/
