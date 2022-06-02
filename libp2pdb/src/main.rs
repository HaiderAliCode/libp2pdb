use libp2p::{
    development_transport,
    futures::StreamExt,
    identity,
    kad::{record::Key, store::MemoryStore, Kademlia, KademliaEvent, QueryResult, Quorum, Record, PeerRecord, PutRecordOk, AddProviderOk},
    mdns::{Mdns, MdnsEvent},
    swarm::{NetworkBehaviourEventProcess, SwarmBuilder, SwarmEvent},
    NetworkBehaviour, PeerId,
};
use std::{error::Error};
use tokio::{self};
use async_std::io::{self, prelude::BufReadExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let id_key = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_key.public());
    println!("local peer id is {:?}", peer_id);

    let transport = development_transport(id_key).await?;

    #[derive(NetworkBehaviour)]
    #[behaviour(event_process = true)]
    struct MyBehaviour {
        kademlia: Kademlia<MemoryStore>,
        mdns: Mdns,
    }

    impl NetworkBehaviourEventProcess<MdnsEvent> for MyBehaviour {
        fn inject_event(&mut self, event: MdnsEvent) {
            if let MdnsEvent::Discovered(list) = event {
                for (peer_id, multiaddrr) in list {
                    self.kademlia.add_address(&peer_id, multiaddrr);
                }
            }
        }
    }

    impl NetworkBehaviourEventProcess<KademliaEvent> for MyBehaviour {
        fn inject_event(&mut self, message: KademliaEvent) {
            match message {
                KademliaEvent::OutboundQueryCompleted { result, .. } => match result {
                    QueryResult::GetProviders(Ok(ok)) => {
                        for peer in ok.providers {
                            println!(
                                "Peer {:?} provides key {:?}",
                                peer,
                                std::str::from_utf8(ok.key.as_ref()).unwrap()
                            );
                        }
                    }
                    QueryResult::GetProviders(Err(err)) => {
                        eprintln!("Failed to get providers: {:?}", err);
                    }
                    QueryResult::GetRecord(Ok(ok)) => {
                        for PeerRecord {
                            record: Record { key, value, .. },
                            ..
                        } in ok.records
                        {
                            println!(
                                "Got record {:?} {:?}",
                                std::str::from_utf8(key.as_ref()).unwrap(),
                                std::str::from_utf8(&value).unwrap(),
                            );
                        }
                    }
                    QueryResult::GetRecord(Err(err)) => {
                        eprintln!("Failed to get record: {:?}", err);
                    }
                    QueryResult::PutRecord(Ok(PutRecordOk { key })) => {
                        println!(
                            "Successfully put record {:?}",
                            std::str::from_utf8(key.as_ref()).unwrap()
                        );
                    }
                    QueryResult::PutRecord(Err(err)) => {
                        eprintln!("Failed to put record: {:?}", err);
                    }
                    QueryResult::StartProviding(Ok(AddProviderOk { key })) => {
                        println!(
                            "Successfully put provider record {:?}",
                            std::str::from_utf8(key.as_ref()).unwrap()
                        );
                    }
                    QueryResult::StartProviding(Err(err)) => {
                        eprintln!("Failed to put provider record: {:?}", err);
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    let mut swarm = {
        let store = MemoryStore::new(peer_id);
        let kademlia = Kademlia::new(peer_id, store);
        let mdns = Mdns::new(Default::default()).await?;
        let behaviour = MyBehaviour { kademlia, mdns };

        SwarmBuilder::new(transport, behaviour, peer_id)
            .executor(Box::new(|fut| {
                tokio::spawn(fut);
            }))
            .build()
    };

     let mut stdin = io::BufReader::new(io::stdin()).lines().fuse();

    // Listen on all interfaces and whatever port the OS assigns.
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    // Kick it off.
    loop {
        tokio::select! {
            line = stdin.select_next_some() => handle_input_line(&mut swarm.behaviour_mut().kademlia, line.expect("Stdin not to close")),
            event = swarm.select_next_some() => match event {
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Listening in {:?}", address);
                },
                _ => {}
            }
        }
    }
}

fn handle_input_line(kademlia: &mut Kademlia<MemoryStore>, line: String) {
    let mut args = line.split(' ');

    match args.next() {
        Some("GET") => {
            let key = {
                match args.next() {
                    Some(key) => Key::new(&key),
                    None => {
                        println!("Expected Key");
                        return;
                    }
                }
            };
            kademlia.get_record(key, Quorum::One);
        }
        Some("GET_PROVIDER") => {
            let key = {
                match args.next() {
                    Some(key) => Key::new(&key),
                    None => {
                        println!("Expected Key");
                        return;
                    }
                }
            };
            kademlia.get_providers(key);
        }
        Some("PUT") => {
            let key = {
                match args.next() {
                    Some(key) => Key::new(&key),
                    None => {
                        println!("Expected Key");
                        return;
                    }
                }
            };
            let value = {
                match args.next() {
                    Some(value) => value.as_bytes().to_vec(),
                    None => {
                        eprintln!("Expected value");
                        return;
                    }
                }
            };
            let record = Record {
                key,
                value,
                publisher: None,
                expires: None,
            };
            kademlia
                .put_record(record, Quorum::One)
                .expect("Failed to store record locally.");
        }
        Some("PUT_PROVIDER") => {
            let key = {
                match args.next() {
                    Some(key) => Key::new(&key),
                    None => {
                        eprintln!("Expected key");
                        return;
                    }
                }
            };

            kademlia
                .start_providing(key)
                .expect("Failed to start providing key");
        }
        _ => {
            println!("Expected GET, GET_PROVIDER, PUT, PUT_PROVIDER");
        }
    }
}
