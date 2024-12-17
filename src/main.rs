use futures::stream::StreamExt;
// futuresクレートのStreamExtトレイトをインポートします。これにより、非同期ストリームを簡単に操作できます。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::time::Duration;
// 標準ライブラリから必要なモジュールをインポートします。ハッシュ計算や文字列のパース、時間の操作に使用します。

use tokio::{io, io::AsyncBufReadExt, select};
// tokioクレートから非同期I/O関連のモジュールをインポートします。非同期での入出力操作や選択を行います。

use anyhow::Result;
// anyhowクレートからResult型をインポートします。エラーハンドリングに使用します。

use tracing_subscriber::EnvFilter;
// tracing_subscriberクレートからEnvFilterをインポートします。ログフィルタリングに使用します。

use libp2p::{kad, noise, tcp, yamux, PeerId, gossipsub, Multiaddr, identify};
// libp2pクレートから必要なモジュールをインポートします。P2Pネットワークの構築に使用します。

use libp2p::identity::{Keypair, PublicKey};
// libp2pクレートからidentityモジュールのKeypairとPublicKeyをインポートします。ピアの識別に使用します。

use libp2p::kad::{GetClosestPeersError, GetClosestPeersOk, PROTOCOL_NAME, QueryResult};
// libp2pクレートのkadモジュールから必要な型をインポートします。Kademliaプロトコルの操作に使用します。

use libp2p::kad::store::MemoryStore;
// libp2pクレートのkadモジュールからMemoryStoreをインポートします。Kademliaのストレージに使用します。

use libp2p::swarm::{NetworkBehaviour, SwarmEvent};
// libp2pクレートのswarmモジュールからNetworkBehaviourとSwarmEventをインポートします。ネットワークの動作とイベント処理に使用します。

#[derive(NetworkBehaviour)]
struct MyBehaviour {
    gossipsub: gossipsub::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    identify: identify::Behaviour
}
// NetworkBehaviourを実装するMyBehaviour構造体を定義します。これにより、複数のプロトコルを一つのビヘイビアとして扱えます。

fn create_kademlia_behavior(local_peer_id: PeerId) -> kad::Behaviour<MemoryStore> {
    // Kademliaのビヘイビアを作成する関数です。
    let mut cfg = kad::Config::default();
    cfg.set_protocol_names(vec![PROTOCOL_NAME.into()]);
    cfg.set_query_timeout(Duration::from_secs(5 * 60));
    let store = MemoryStore::new(local_peer_id);
    kad::Behaviour::with_config(local_peer_id, store, cfg)
}
// Kademliaの設定を行い、ビヘイビアを返します。

fn create_identify_behavior(local_public_key: PublicKey) -> identify::Behaviour {
    // Identifyのビヘイビアを作成する関数です。
    let id_cfg = identify::Config::new(identify::PROTOCOL_NAME.to_string(),local_public_key);
    println!("{:?}",identify::PROTOCOL_NAME.to_string());
    identify::Behaviour::new(id_cfg)
}
// Identifyの設定を行い、ビヘイビアを返します。

fn create_gossipsub_behavior(id_keys: Keypair) -> gossipsub::Behaviour {
    // Gossipsubのビヘイビアを作成する関数です。
    let message_id_fn = |message: &gossipsub::Message| {
        let mut s = DefaultHasher::new();
        message.data.hash(&mut s);
        gossipsub::MessageId::from(s.finish().to_string())
    };
    // メッセージIDを生成する関数を定義します。

    let gossipsub_config = gossipsub::ConfigBuilder::default()
        .heartbeat_interval(Duration::from_secs(10))
        .validation_mode(gossipsub::ValidationMode::Strict)
        .message_id_fn(message_id_fn)
        .build()
        .map_err(|msg| io::Error::new(io::ErrorKind::Other, msg)).unwrap();
    // Gossipsubの設定を行います。

    gossipsub::Behaviour::new(
        gossipsub::MessageAuthenticity::Signed(id_keys),
        gossipsub_config,
    ).unwrap()
}
// Gossipsubのビヘイビアを返します。

#[tokio::main]
async fn main() -> Result<()> {
    // メイン関数です。非同期で実行されます。
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    // ログの設定を行います。

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_dns()?
        .with_behaviour(|key| {
            Ok(MyBehaviour { gossipsub: create_gossipsub_behavior(key.clone()),
                            kademlia: create_kademlia_behavior(key.public().to_peer_id()) ,
                            identify: create_identify_behavior(key.public())})
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(5)))
        .build();
    // Swarmの設定を行います。

    println!("My peer addr is {:?}",swarm.local_peer_id());
    // 自分のピアアドレスを表示します。

    let topic = gossipsub::IdentTopic::new("test-net");
    // Gossipsubのトピックを作成します。

    swarm.behaviour_mut().gossipsub.subscribe(&topic)?;
    // トピックにサブスクライブします。

    let bootnodes: [(Multiaddr,PeerId); 1] = [
        (Multiaddr::from_str("/ip6/::1/tcp/33749")?,PeerId::from_str("12D3KooWBk8UF3zfZJb7r8NraocrvEY2vwvFZfCnGWRG7Zkzs8Ji")?),
    ];
    // ブートノードのアドレスとピアIDを設定します。

    for (peer_addr, peer_id) in bootnodes {
        match swarm.dial(peer_addr.clone()) {
            Ok(_) => println!("Dialing peer {:?}", peer_addr),
            Err(e) => eprintln!("Failed to dial peer {:?}: {:?}", peer_addr, e),
        }
        println!("adding initial peer addr to kademli: routing table: {peer_addr}");
        swarm
            .behaviour_mut()
            .kademlia
            .add_address(&peer_id, peer_addr.clone());
        swarm
            .behaviour_mut()
            .identify
            .push(std::iter::once(peer_id));
    }
    // ブートノードに接続し、Kademliaのルーティングテーブルに追加します。

    let to_search: PeerId = Keypair::generate_ed25519().public().into();
    println!("Searching for the closest peers to {:?}", to_search);
    swarm.behaviour_mut().kademlia.get_closest_peers(to_search);
    // 最も近いピアを検索します。

    swarm.listen_on("/ip6/::0/tcp/0".parse()?)?;
    // 全てのインターフェースでリッスンします。

    let mut stdin = io::BufReader::new(io::stdin()).lines();
    println!("Enter messages via STDIN and they will be sent to connected peers using Gossipsub");
    // 標準入力からのメッセージを読み取ります。

    loop {
        select! {
            Ok(Some(line)) = stdin.next_line() => {
                if let Err(e) = swarm
                    .behaviour_mut().gossipsub
                    .publish(topic.clone(), line.as_bytes()) {
                    println!("Publish error: {e:?}");
                }
            }
            event = swarm.select_next_some() => match event  {
                SwarmEvent::Behaviour(MyBehaviourEvent::Kademlia(kad::Event::OutboundQueryProgressed {
                result: QueryResult::GetClosestPeers(result),
                ..
            })) => {
                match result {
                    Ok(GetClosestPeersOk { key: _, peers }) => {
                        if !peers.is_empty() {
                            println!("Query finished with closest peers: {:#?}", peers);
                            for peer in peers {
                                println!("gossipsub adding peer {peer}");
                                swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer);
                            }
                        } else {
                            println!("Query finished with no closest peers.")
                        }
                    }
                    Err(GetClosestPeersError::Timeout { peers, .. }) => {
                        if !peers.is_empty() {
                            println!("Query timed out with closest peers: {:#?}", peers);
                            for peer in peers {
                                println!("gossipsub adding peer {peer}");
                                swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer);
                            }
                        } else {
                            println!("Query timed out with no closest peers.");
                        }
                    }
                };
            }
            SwarmEvent::Behaviour(MyBehaviourEvent::Identify(identify::Event::Received {
                peer_id,
                info:
                    identify::Info {
                        listen_addrs,
                        protocols,
                        ..
                    },
            })) => {
                if protocols
                    .iter()
                    .any(|p| *p == kad::PROTOCOL_NAME)
                {
                    for addr in listen_addrs {
                        println!("received addr {addr} trough identify");
                        swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                    }
                } else {
                    println!("something funky happened, investigate it");
                }
            }

            SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source: peer_id,
                message_id: id,
                message,
            })) => println!(
                    "Got message: '{}'with id: {id} from peer: {peer_id}",
                    String::from_utf8_lossy(&message.data),
                ),
            SwarmEvent::NewListenAddr { address, .. } => {
                println!("Local node is listening on {address}");
            }
            _ => println!("{:?}", event),
            }
        }
    }
}
// イベントループを開始し、標準入力からのメッセージをGossipsubで送信したり、Swarmのイベントを処理します。
