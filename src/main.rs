#![allow(unused)]

mod ring_buffer;
use crossbeam_channel::{select, select_biased, unbounded, Receiver, Sender};
use log::{error, info, trace, warn, LevelFilter};
use rand::distributions::Bernoulli;
use rand::prelude::*;
use ring_buffer::RingBuffer;
use std::collections::HashMap;
use std::{default, fs, thread};
use wg_2024::config::Config;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::*;
use wg_2024::tests::*;

const RING_BUFF_SZ: usize = 64;

// TODO handle c.send() errors, just log the error I guess

#[derive(Debug, PartialEq)]
enum State {
    Running,
    Crashing,
}

impl State {
    #[inline]
    fn is_running(&self) -> bool {
        match self {
            State::Running => true,
            State::Crashing => false,
        }
    }

    #[inline]
    fn is_crashing(&self) -> bool {
        !self.is_running()
    }
}

#[derive(Debug)]
pub struct MyDrone {
    id: NodeId,
    log_channel: String,
    controller_send: Sender<DroneEvent>,
    controller_recv: Receiver<DroneCommand>,
    packet_recv: Receiver<Packet>,
    pdr_distribution: Bernoulli,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    state: State,
    // ! TODO use hashmap if necessary
    flood_history: RingBuffer<(NodeId, u64)>,
}

impl Drone for MyDrone {
    fn new(
        id: NodeId,
        controller_send: Sender<DroneEvent>,
        controller_recv: Receiver<DroneCommand>,
        packet_recv: Receiver<Packet>,
        packet_send: HashMap<NodeId, Sender<Packet>>,
        pdr: f32,
    ) -> Self {
        if (cfg!(test)) {
            pretty_env_logger::formatted_builder()
                .filter_level(LevelFilter::Info)
                .is_test(true)
                .try_init();
        } else {
            pretty_env_logger::try_init();
        }

        let log_channel: String = format!("CppEnjoyers[{}]", id);
        let mut pdr_distribution: Bernoulli =
            Bernoulli::new(pdr as f64).unwrap_or_else(|e: rand::distributions::BernoulliError| {
                warn!(target: &log_channel, "Invalid PDR {}, setting to 0.0", pdr);
                Bernoulli::new(0.).unwrap()
            });
        Self {
            id,
            log_channel,
            controller_send,
            controller_recv,
            packet_recv,
            packet_send,
            pdr_distribution,
            state: State::Running,
            flood_history: RingBuffer::new_with_size(RING_BUFF_SZ),
        }
    }

    fn run(&mut self) {
        info!(target: &self.log_channel, "Drone started");
        while self.is_running() {
            select_biased! {
                recv(self.controller_recv) -> command => {
                    if let Ok(command) = command {
                        info!(target: &self.log_channel, "Processing command: {:?}", command);
                        self.handle_command(command);
                    }
                },
                recv(self.packet_recv) -> packet => {
                    if let Ok(packet) = packet {
                        info!(target: &self.log_channel, "Processing packet: {:?}", packet);
                        self.handle_packet(packet);
                    }
                },
            }
        }
        loop {
            select! {
                recv(self.packet_recv) -> packet => {
                    if let Ok(packet) = packet {
                        info!(target: &self.log_channel, "Processing packet during crash: {:?}", packet);
                        self.handle_crash(packet);
                    }
                },
                default => {
                    info!(target: &self.log_channel, "Crashing gently");
                    break;
                }
            }
        }
    }
}

impl MyDrone {
    fn send_nack(&self, mut packet: Packet, nack_type: NackType) {
        let mut source_header: SourceRoutingHeader = match packet
            .routing_header
            .sub_route(0..packet.routing_header.hop_index) {
                Some(sh) => sh,
                None => {
                    error!(target: &self.log_channel, "\tCan't invert source routing header: {}. Dropping Nack packet", packet.routing_header);
                    return;
                }
            };
        source_header.reset_hop_index();
        source_header.reverse();
        source_header.hop_index = 1;

        // If packet type is Ack,Nack,FloodResponse, send original packet to sim controller
        // else send back nack
        match packet.pack_type {
            // send original packet to simulation controller
            PacketType::Nack(_) | PacketType::Ack(_) | PacketType::FloodResponse(_) => {
                info!(target: &self.log_channel, "\tError in original Ack/Nack/FloodREsponse packet, forwarding to controller");
                packet.routing_header.decrease_hop_index();
                self.controller_send
                    .send(DroneEvent::ControllerShortcut(packet));
            }
            // Send nack back
            PacketType::MsgFragment(f) => {
                if (source_header.hops.len() < 2) {
                    warn!(target: &self.log_channel, "\tInverted routing header {} too small, can't send. Dropping Nack packet", source_header);
                    return;
                }
                self.packet_send
                    .get(&source_header.current_hop().unwrap_or_else(|| { error!(target: &self.log_channel, "Can't get current hop. You probably found a bug :("); panic!(); }))
                    .map_or_else(
                        || error!(target: &self.log_channel, "\tMissing sender channel for Nack packet, topology error?. Dropping Nack packet"),
                        |s: &Sender<Packet>| {
                            let nack: Packet = Packet::new_nack(
                                source_header,
                                packet.session_id,
                                Nack {
                                    fragment_index: f.fragment_index,
                                    nack_type,
                                },
                            );
                            info!(target: &self.log_channel, "\tLogging and sending Nack packet: {:?}", nack);
                            self.controller_send
                                .send(DroneEvent::PacketSent(nack.clone()));
                            s.send(nack);
                        },
                    )
            }
            PacketType::FloodRequest(_) => {
                error!(target: &self.log_channel, "\tFound a flood request while trying to send back Nack. You probably found a bug :(");
                unreachable!()
            }
        }
    }

    fn handle_packet(&mut self, mut packet: Packet) {
        if let PacketType::FloodRequest(fr) = packet.pack_type {
            info!(target: &self.log_channel, "\tHandling flood request");
            let Packet {
                routing_header,
                session_id,
                ..
            } = packet;
            self.handle_flood_request(routing_header, session_id, fr);
            return;
        }

        // STEP 2
        // ** done here so it's not needed later **
        packet.routing_header.increase_hop_index();

        // STEP1 and STEP3
        // NB this could be optimized by swapping the ifs and directly accessing current_hop
        // but it's less clean than this so decide later
        let opt: Option<NodeId> = packet.routing_header.previous_hop();
        if opt.is_none_or(|v: NodeId| v != self.id) {
            info!(target: &self.log_channel, "\tCreating Nack - Unexpected recipient. My id: {}", self.id);
            self.send_nack(packet, NackType::UnexpectedRecipient(self.id));
            return;
        }
        let next_hop: Option<NodeId> = packet.routing_header.current_hop();
        match next_hop {
            // STEP 4
            Some(next_hop) => {
                let next_hop_drone_id: NodeId = next_hop;
                let next_hop_channel: Option<&Sender<Packet>> =
                    self.packet_send.get(&next_hop_drone_id);

                if let Some(next_hop_channel) = next_hop_channel {
                    self.send_packet(packet, next_hop_channel);
                } else {
                    info!(target: &self.log_channel, "\tCreating Nack - Error in routing");
                    self.send_nack(packet, NackType::ErrorInRouting(next_hop_drone_id));
                }
            }
            None => {
                info!(target: & self.log_channel, "\tCreating Nack - Drone is destination");
                self.send_nack(packet, NackType::DestinationIsDrone);
            }
        }
    }

    fn send_packet(&self, packet: Packet, channel: &Sender<Packet>) {
        match packet.pack_type {
            PacketType::Nack(_) | PacketType::Ack(_) => {
                self.controller_send
                    .send(DroneEvent::PacketSent(packet.clone()));
                info!(target: &self.log_channel, "\tLogging and sending packet to {}", packet.routing_header.current_hop().expect("If we panic here there's a bug :("));
                channel.send(packet);
            }
            PacketType::MsgFragment(_) => {
                if self.pdr_distribution.sample(&mut rand::thread_rng()) {
                    self.controller_send
                        .send(DroneEvent::PacketDropped(packet.clone()));
                    info!(target: &self.log_channel, "\tPacket fragment dropped! Creating Nack");
                    self.send_nack(packet, NackType::Dropped);
                } else {
                    self.controller_send
                        .send(DroneEvent::PacketSent(packet.clone()));
                    info!(target: &self.log_channel, "\tLogging and sending fragment to: {}", packet.routing_header.current_hop().expect("If we panic here there's a bug :("));
                    channel.send(packet);
                }
            }
            PacketType::FloodResponse(_) => {
                self.controller_send
                    .send(DroneEvent::PacketSent(packet.clone()));
                info!(target: &self.log_channel, "\tLogging and sending flood response to: {}", packet.routing_header.current_hop().expect("If we panic here there's a bug :("));
                channel.send(packet);
            }
            PacketType::FloodRequest(_) => {
                self.controller_send.send(DroneEvent::PacketSent(packet.clone()));
                info!(target: &self.log_channel, "\tLogging and forwarding flood request: {}", packet);
                channel.send(packet);
            }
        }
    }

    fn handle_flood_request(
        &mut self,
        routing_header: SourceRoutingHeader,
        sid: u64,
        mut flood_r: FloodRequest,
    ) {
        let sender_tuple: (NodeId, NodeType) = match flood_r.path_trace.last() {
            Some(tup) => *tup,
            None => {
                warn!(target: &self.log_channel, "\tReceived flood request with empty path trace. Assuming it arrived from the initiator");
                (flood_r.initiator_id, NodeType::Client) // assume it's a client, who cares
            }
        };

        let (sender_id, _) = sender_tuple;
        flood_r.increment(self.id, NodeType::Drone);

        if (self
            .flood_history
            .contains(&(flood_r.initiator_id, flood_r.flood_id))
            || self.packet_send.len() <= 1)
        // cargo fmt is clearly bonkers
        {
            let mut new_packet: Packet = flood_r.generate_response(sid);
            info!(target: &self.log_channel, "\tGenerated flood response: {}", new_packet);
            new_packet.routing_header.increase_hop_index();
            let next_hop: NodeId = new_packet.routing_header.current_hop().expect("If this panics here the WG code is bugged");
            self.packet_send.get(&next_hop).map_or_else(
                || {
                    error!(target: &self.log_channel, "\tCan't find sender channel for flood response. Dropping packet");
                } ,
                |c: &Sender<Packet>| {
                    self.send_packet(new_packet, c);
                },
            );
            return;
        }

        // TODO add the check if the neighbour exists in debug mode
        self.flood_history
            .push((flood_r.initiator_id, flood_r.flood_id));
        self.packet_send.iter().for_each(|(id, c)| {
            if *id != sender_id {
                let new_packet =
                Packet::new_flood_request(routing_header.clone(), sid, flood_r.clone());
                self.send_packet(new_packet, c);
            }
        });
    }

    fn handle_command(&mut self, command: DroneCommand) {
        match command {
            DroneCommand::AddSender(node_id, sender) => {
                info!(target: &self.log_channel, "\tAdding sender channel to {}", node_id);
                self.packet_send.insert(node_id, sender);
            }
            DroneCommand::SetPacketDropRate(pdr) => {
                if let Ok(new_pdr) = Bernoulli::new(pdr as f64) {
                    info!(target: &self.log_channel, "\tSetting new PDR: {}", pdr);
                    self.pdr_distribution = new_pdr;
                } else {
                    warn!(target: &self.log_channel, "\tInvalid PDR: {}, keeping old value", pdr);
                }
            }
            DroneCommand::Crash => {
                info!(target: &self.log_channel, "\tStarting crash routine");
                self.state = State::Crashing
            }
            DroneCommand::RemoveSender(node_id) => {
                info!(target: &self.log_channel, "\tRemoving sender channel to id: {}", node_id);
                self.packet_send.remove(&node_id);
            }
        }
    }

    fn handle_crash(&mut self, mut packet: Packet) {
        match packet.pack_type {
            PacketType::Nack(_) | PacketType::Ack(_) | PacketType::FloodResponse(_) => {
                self.handle_packet(packet);
            }
            PacketType::FloodRequest(_) => {
                info!(target: &self.log_channel, "\tGot flood request while crashing, ignoring");
            }
            PacketType::MsgFragment(_) => {
                packet.routing_header.increase_hop_index();
                info!(target: &self.log_channel, "\tCrashing, sending Nack error in routing, can't process fragment",);
                self.send_nack(packet, NackType::ErrorInRouting(self.id));
            }
        }
    }

    #[inline]
    fn is_running(&self) -> bool {
        self.state.is_running()
    }

    #[inline]
    fn is_crashing(&self) -> bool {
        self.state.is_crashing()
    }
}

#[cfg(test)]
mod drone_tests {
    use wg_2024::{config::Client, tests};

    use crate::*;

    #[test]
    fn test_fragment_drop() {
        wg_2024::tests::generic_fragment_drop::<MyDrone>();
    }

    #[test]
    fn test_fragment_forward() {
        wg_2024::tests::generic_fragment_forward::<MyDrone>();
    }

    #[test]
    fn test_flooding_simple_topology() {
        // Client<1> channels
        let (c_send, c_recv) = unbounded();
        // Drone 11
        let (d_send, d_recv) = unbounded();
        // Drone 12
        let (d12_send, d12_recv) = unbounded();
        // Drone 13
        let (d13_send, d13_recv) = unbounded();
        // Drone 14
        let (d14_send, d14_recv) = unbounded();
        // SC - needed to not make the drone crash
        let (_d_command_send, d_command_recv) = unbounded();

        // Drone 11
        let neighbours11 = HashMap::from([
            (12, d12_send.clone()),
            (13, d13_send.clone()),
            (14, d14_send.clone()),
            (1, c_send.clone()),
        ]);
        let mut drone = MyDrone::new(
            11,
            unbounded().0,
            d_command_recv.clone(),
            d_recv.clone(),
            neighbours11,
            0.0,
        );
        // Drone 12
        let neighbours12 = HashMap::from([(11, d_send.clone())]);
        let mut drone2 = MyDrone::new(
            12,
            unbounded().0,
            d_command_recv.clone(),
            d12_recv.clone(),
            neighbours12,
            0.0,
        );
        // Drone 13
        let neighbours13 = HashMap::from([(11, d_send.clone()), (14, d14_send.clone())]);
        let mut drone3 = MyDrone::new(
            13,
            unbounded().0,
            d_command_recv.clone(),
            d13_recv.clone(),
            neighbours13,
            0.0,
        );
        // Drone 14
        let neighbours14 = HashMap::from([(11, d_send.clone()), (13, d13_send.clone())]);
        let mut drone4 = MyDrone::new(
            14,
            unbounded().0,
            d_command_recv.clone(),
            d14_recv.clone(),
            neighbours14,
            0.0,
        );

        // Spawn the drone's run method in a separate thread
        thread::spawn(move || {
            drone.run();
        });

        thread::spawn(move || {
            drone2.run();
        });

        thread::spawn(move || {
            drone3.run();
        });

        thread::spawn(move || {
            drone4.run();
        });

        let mut msg = Packet::new_flood_request(
            SourceRoutingHeader::new(vec![], 15),
            23,
            FloodRequest::initialize(56, 1, NodeType::Client),
        );

        // "Client" sends packet to the drone
        d_send.send(msg.clone()).unwrap();

        for _ in 0..3 {
            match c_recv.recv().unwrap().pack_type {
                PacketType::FloodResponse(f) => {
                    println!("{:?}", f.path_trace);
                }
                _ => {}
            }
        }

        // assert_eq!(
        //     c_recv.recv().unwrap(),
        //     Packet {
        //         pack_type: PacketType::FloodResponse(FloodResponse { flood_id: 56, path_trace: vec![(1, NodeType::Client), (11, NodeType::Drone), (12, NodeType::Drone)] },),
        //         routing_header: SourceRoutingHeader {
        //             hop_index: 2,
        //             hops: vec![12, 11, 1],
        //         },
        //         session_id: 23,
        //     }
        // );
    }

    #[test]
    fn test_destination_is_drone() {
        // Client<1> channels
        let (c_send, c_recv) = unbounded();
        // Drone 11
        let (d_send, d_recv) = unbounded();
        // Drone 12
        let (d12_send, d12_recv) = unbounded();
        // SC - needed to not make the drone crash
        let (_d_command_send, d_command_recv) = unbounded();

        // Drone 11
        let neighbours11 = HashMap::from([(12, d12_send.clone()), (1, c_send.clone())]);
        let mut drone = MyDrone::new(
            11,
            unbounded().0,
            d_command_recv.clone(),
            d_recv.clone(),
            neighbours11,
            0.0,
        );
        // Drone 12
        let neighbours12 = HashMap::from([(11, d_send.clone())]);
        let mut drone2 = MyDrone::new(
            12,
            unbounded().0,
            d_command_recv.clone(),
            d12_recv.clone(),
            neighbours12,
            0.0,
        );

        // Spawn the drone's run method in a separate thread
        thread::spawn(move || {
            drone.run();
        });

        thread::spawn(move || {
            drone2.run();
        });

        let mut msg = Packet::new_fragment(
            SourceRoutingHeader::new(vec![1, 11, 12], 1),
            56,
            Fragment {
                fragment_index: 1,
                total_n_fragments: 1,
                length: 128,
                data: [1; 128],
            },
        );

        // "Client" sends packet to the drone
        d_send.send(msg.clone()).unwrap();

        // Client receive an ACK originated from 'd2'
        assert_eq!(
            c_recv.recv().unwrap(),
            Packet {
                routing_header: SourceRoutingHeader::new(vec![12, 11, 1], 2),
                pack_type: PacketType::Nack(Nack {
                    fragment_index: 1,
                    nack_type: NackType::DestinationIsDrone
                }),
                session_id: 56,
            }
        );
    }

    #[test]
    fn test_unexpected_recipient() {
        // Client<1> channels
        let (c_send, c_recv) = unbounded();
        // Drone 11
        let (d_send, d_recv) = unbounded();
        // Drone 12
        let (d12_send, d12_recv) = unbounded();
        // SC - needed to not make the drone crash
        let (_d_command_send, d_command_recv) = unbounded();

        // Drone 11
        let neighbours11 = HashMap::from([(12, d12_send.clone()), (1, c_send.clone())]);
        let mut drone = MyDrone::new(
            11,
            unbounded().0,
            d_command_recv.clone(),
            d_recv.clone(),
            neighbours11,
            0.0,
        );
        // Drone 12
        let neighbours12 = HashMap::from([(11, d_send.clone())]);
        let mut drone2 = MyDrone::new(
            12,
            unbounded().0,
            d_command_recv.clone(),
            d12_recv.clone(),
            neighbours12,
            0.0,
        );

        // Spawn the drone's run method in a separate thread
        thread::spawn(move || {
            drone.run();
        });

        thread::spawn(move || {
            drone2.run();
        });

        let mut msg = Packet::new_fragment(
            SourceRoutingHeader::new(vec![1, 45, 12], 1),
            56,
            Fragment {
                fragment_index: 1,
                total_n_fragments: 1,
                length: 128,
                data: [1; 128],
            },
        );

        // "Client" sends packet to the drone
        d_send.send(msg.clone()).unwrap();

        // Client receive an ACK originated from 'd2'
        assert_eq!(
            c_recv.recv().unwrap(),
            Packet {
                routing_header: SourceRoutingHeader::new(vec![45, 1], 1),
                pack_type: PacketType::Nack(Nack {
                    fragment_index: 1,
                    nack_type: NackType::UnexpectedRecipient(11)
                }),
                session_id: 56,
            }
        );
    }

    #[test]
    fn test_error_in_routing() {
        // Client<1> channels
        let (c_send, c_recv) = unbounded();
        // Drone 11
        let (d_send, d_recv) = unbounded();
        // Drone 12
        let (d12_send, d12_recv) = unbounded();
        // SC - needed to not make the drone crash
        let (_d_command_send, d_command_recv) = unbounded();

        // Drone 11
        let neighbours11 = HashMap::from([(12, d12_send.clone()), (1, c_send.clone())]);
        let mut drone = MyDrone::new(
            11,
            unbounded().0,
            d_command_recv.clone(),
            d_recv.clone(),
            neighbours11,
            0.0,
        );
        // Drone 12
        let neighbours12 = HashMap::from([(11, d_send.clone())]);
        let mut drone2 = MyDrone::new(
            12,
            unbounded().0,
            d_command_recv.clone(),
            d12_recv.clone(),
            neighbours12,
            0.0,
        );

        // Spawn the drone's run method in a separate thread
        thread::spawn(move || {
            drone.run();
        });

        thread::spawn(move || {
            drone2.run();
        });

        let mut msg = Packet::new_fragment(
            SourceRoutingHeader::new(vec![1, 11, 45], 1),
            56,
            Fragment {
                fragment_index: 1,
                total_n_fragments: 1,
                length: 128,
                data: [1; 128],
            },
        );

        // "Client" sends packet to the drone
        d_send.send(msg.clone()).unwrap();

        // Client receive an ACK originated from 'd2'
        assert_eq!(
            c_recv.recv().unwrap(),
            Packet {
                routing_header: SourceRoutingHeader::new(vec![11, 1], 1),
                pack_type: PacketType::Nack(Nack {
                    fragment_index: 1,
                    nack_type: NackType::ErrorInRouting(45)
                }),
                session_id: 56,
            }
        );
    }

    #[test]
    fn test_nack_error_in_routing() {
        // Client<1> channels
        let (c_send, c_recv) = unbounded();
        // Drone 11
        let (d_send, d_recv) = unbounded();
        // Drone 12
        let (d12_send, d12_recv) = unbounded();
        // SC - needed to not make the drone crash
        let (tx_event, rx_event) = unbounded::<DroneEvent>();
        let (tx_cmd, rx_cmd) = unbounded::<DroneCommand>();

        // Drone 11
        let neighbours11 = HashMap::from([(12, d12_send.clone()), (1, c_send.clone())]);
        let mut drone = MyDrone::new(
            11,
            tx_event.clone(),
            rx_cmd.clone(),
            d_recv.clone(),
            neighbours11,
            0.0,
        );
        // Drone 12
        let neighbours12 = HashMap::from([(11, d_send.clone())]);
        let mut drone2 = MyDrone::new(
            12,
            tx_event.clone(),
            rx_cmd.clone(),
            d12_recv.clone(),
            neighbours12,
            0.0,
        );
        // Spawn the drone's run method in a separate thread
        thread::spawn(move || {
            drone.run();
        });
        /*
                thread::spawn(move || {
                    drone2.run();
                });
        */
        let msg = Packet::new_nack(
            SourceRoutingHeader::new(vec![1, 11, 45], 1),
            56,
            Nack {
                fragment_index: 1,
                nack_type: NackType::Dropped,
            },
        );

        // "Client" sends packet to the drone
        d_send.send(msg.clone()).unwrap();
        assert_eq!(
            rx_event.recv().unwrap(),
            DroneEvent::ControllerShortcut(Packet::new_nack(
                SourceRoutingHeader::new(vec![1, 11, 45], 1),
                56,
                Nack {
                    fragment_index: 1,
                    nack_type: NackType::Dropped
                }
            ))
        );
    }

    #[test]
    fn test_ack_error_in_routing() {
        // Client<1> channels
        let (c_send, c_recv) = unbounded();
        // Drone 11
        let (d_send, d_recv) = unbounded();
        // Drone 12
        let (d12_send, d12_recv) = unbounded();
        // SC - needed to not make the drone crash
        let (tx_event, rx_event) = unbounded::<DroneEvent>();
        let (tx_cmd, rx_cmd) = unbounded::<DroneCommand>();

        // Drone 11
        let neighbours11 = HashMap::from([(12, d12_send.clone()), (1, c_send.clone())]);
        let mut drone = MyDrone::new(
            11,
            tx_event.clone(),
            rx_cmd.clone(),
            d_recv.clone(),
            neighbours11,
            0.0,
        );
        // Drone 12
        let neighbours12 = HashMap::from([(11, d_send.clone())]);
        let mut drone2 = MyDrone::new(
            12,
            tx_event.clone(),
            rx_cmd.clone(),
            d12_recv.clone(),
            neighbours12,
            0.0,
        );
        // Spawn the drone's run method in a separate thread
        thread::spawn(move || {
            drone.run();
        });
        /*
                thread::spawn(move || {
                    drone2.run();
                });
        */
        let msg = Packet::new_ack(SourceRoutingHeader::new(vec![1, 11, 45], 1), 56, 1);

        // "Client" sends packet to the drone
        d_send.send(msg.clone()).unwrap();
        assert_eq!(
            rx_event.recv().unwrap(),
            DroneEvent::ControllerShortcut(Packet::new_ack(
                SourceRoutingHeader::new(vec![1, 11, 45], 1),
                56,
                1
            ))
        );
    }

    #[test]
    fn test_flood_response_error_in_routing() {
        // Client<1> channels
        let (c_send, c_recv) = unbounded();
        // Drone 11
        let (d_send, d_recv) = unbounded();
        // Drone 12
        let (d12_send, d12_recv) = unbounded();
        // SC - needed to not make the drone crash
        let (tx_event, rx_event) = unbounded::<DroneEvent>();
        let (tx_cmd, rx_cmd) = unbounded::<DroneCommand>();

        // Drone 11
        let neighbours11 = HashMap::from([(12, d12_send.clone()), (1, c_send.clone())]);
        let mut drone = MyDrone::new(
            11,
            tx_event.clone(),
            rx_cmd.clone(),
            d_recv.clone(),
            neighbours11,
            0.0,
        );
        // Drone 12
        let neighbours12 = HashMap::from([(11, d_send.clone())]);
        let mut drone2 = MyDrone::new(
            12,
            tx_event.clone(),
            rx_cmd.clone(),
            d12_recv.clone(),
            neighbours12,
            0.0,
        );
        // Spawn the drone's run method in a separate thread
        thread::spawn(move || {
            drone.run();
        });
        /*
            thread::spawn(move || {
                drone2.run();
            });
        */
        let msg = Packet::new_flood_response(
            SourceRoutingHeader::new(vec![1, 11, 45], 1),
            56,
            FloodResponse {
                flood_id: 1,
                path_trace: vec![
                    (80, NodeType::Drone),
                    (90, NodeType::Drone),
                    (99, NodeType::Drone),
                ],
            },
        );

        // "Client" sends packet to the drone
        d_send.send(msg.clone()).unwrap();
        assert_eq!(
            rx_event.recv().unwrap(),
            DroneEvent::ControllerShortcut(msg)
        )
    }

    #[test]
    fn test_add_new_drone() {
        // Client<1> channels
        let (c_send, c_recv) = unbounded();
        // Drone 11
        let (d_send, d_recv) = unbounded();
        // Drone 12
        let (d12_send, d12_recv) = unbounded();
        // SC - needed to not make the drone crash
        let (tx_event, rx_event) = unbounded::<DroneEvent>();
        let (tx_cmd, rx_cmd) = unbounded::<DroneCommand>();
        let (tx_cmd2, rx_cmd2) = unbounded::<DroneCommand>();

        // Drone 11
        let neighbours11 = HashMap::from([(12, d12_send.clone()), (1, c_send.clone())]);
        let mut drone = MyDrone::new(
            11,
            tx_event.clone(),
            rx_cmd.clone(),
            d_recv.clone(),
            HashMap::new(),
            0.0,
        );
        // Drone 12
        let neighbours12 = HashMap::from([(11, d_send.clone())]);
        let mut drone2 = MyDrone::new(
            12,
            tx_event.clone(),
            rx_cmd2.clone(),
            d12_recv.clone(),
            HashMap::new(),
            0.0,
        );
        // Spawn the drone's run method in a separate thread
        thread::spawn(move || {
            drone.run();
        });

        thread::spawn(move || {
            drone2.run();
        });

        tx_cmd.send(DroneCommand::AddSender(12, d12_send.clone()));
        tx_cmd.send(DroneCommand::AddSender(1, c_send.clone()));
        tx_cmd2.send(DroneCommand::AddSender(11, d_send.clone()));

        let mut msg = Packet::new_flood_request(
            SourceRoutingHeader::new(vec![], 15),
            23,
            FloodRequest::initialize(56, 1, NodeType::Client),
        );

        // "Client" sends packet to the drone
        d_send.send(msg.clone()).unwrap();

        match c_recv.recv().unwrap().pack_type {
            PacketType::FloodResponse(f) => {
                println!("{:?}", f.path_trace);
            }
            _ => {}
        }

    }

    #[test]
    fn test_remove_drone() {
        // Client<1> channels
        let (c_send, c_recv) = unbounded();
        // Drone 11
        let (d_send, d_recv) = unbounded();
        // Drone 12
        let (d12_send, d12_recv) = unbounded();
        // SC - needed to not make the drone crash
        let (tx_event, rx_event) = unbounded::<DroneEvent>();
        let (tx_cmd, rx_cmd) = unbounded::<DroneCommand>();
        let (tx_cmd2, rx_cmd2) = unbounded::<DroneCommand>();

        // Drone 11
        let neighbours11 = HashMap::from([(12, d12_send.clone()), (1, c_send.clone())]);
        let mut drone = MyDrone::new(
            11,
            tx_event.clone(),
            rx_cmd.clone(),
            d_recv.clone(),
            neighbours11,
            0.0,
        );
        // Drone 12
        let neighbours12 = HashMap::from([(11, d_send.clone())]);
        let mut drone2 = MyDrone::new(
            12,
            tx_event.clone(),
            rx_cmd2.clone(),
            d12_recv.clone(),
            neighbours12,
            0.0,
        );
        // Spawn the drone's run method in a separate thread
        thread::spawn(move || {
            drone.run();
        });

        thread::spawn(move || {
            drone2.run();
        });

        tx_cmd.send(DroneCommand::RemoveSender(12));
        tx_cmd2.send(DroneCommand::RemoveSender(11));

        let mut msg = Packet::new_flood_request(
            SourceRoutingHeader::new(vec![], 15),
            23,
            FloodRequest::initialize(56, 1, NodeType::Client),
        );

        // "Client" sends packet to the drone
        d_send.send(msg.clone()).unwrap();

        match c_recv.recv().unwrap().pack_type {
            PacketType::FloodResponse(f) => {
                println!("{:?}", f.path_trace);
            }
            _ => {}
        }

    }

    #[test]
    fn test_set_pdr(){
        // Client<1> channels
        let (c_send, c_recv) = unbounded();
        // Drone 11
        let (d_send, d_recv) = unbounded();
        // Drone 12
        let (d12_send, d12_recv) = unbounded();
        // SC - needed to not make the drone crash
        let (_d_command_send, d_command_recv) = unbounded();

        // Drone 11
        let neighbours11 = HashMap::from([(12, d12_send.clone()), (1, c_send.clone())]);
        let mut drone = MyDrone::new(
            11,
            unbounded().0,
            d_command_recv.clone(),
            d_recv.clone(),
            neighbours11,
            0.0,
        );
        // Drone 12
        let neighbours12 = HashMap::from([(11, d_send.clone())]);
        let mut drone2 = MyDrone::new(
            12,
            unbounded().0,
            unbounded().1,
            d12_recv.clone(),
            neighbours12,
            0.0,
        );

        // Spawn the drone's run method in a separate thread
        thread::spawn(move || {
            drone.run();
        });

        thread::spawn(move || {
            drone2.run();
        });

        _d_command_send.send(DroneCommand::SetPacketDropRate(1.));

        let msg = Packet {
            pack_type: PacketType::MsgFragment(Fragment {
                fragment_index: 1,
                total_n_fragments: 1,
                length: 128,
                data: [1; 128],
            }),
            routing_header: SourceRoutingHeader {
                hop_index: 1,
                hops: vec![1, 11, 12],
            },
            session_id: 56,
        };

        // "Client" sends packet to the drone
        d_send.send(msg.clone()).unwrap();

        let dropped = Nack {
            fragment_index: 1,
            nack_type: NackType::Dropped,
        };
        let srh = SourceRoutingHeader {
            hop_index: 1,
            hops: vec![11, 1],
        };
        let nack_packet = Packet {
            pack_type: PacketType::Nack(dropped),
            routing_header: srh,
            session_id: 56,
        };

        // Client listens for packet from the drone (Dropped Nack)
        assert_eq!(c_recv.recv().unwrap(), nack_packet);
    }

    #[test]
    fn test_crash(){
        // Client<1> channels
        let (c_send, c_recv) = unbounded();
        // Drone 11
        let (d_send, d_recv) = unbounded();
        // Drone 12
        let (d12_send, d12_recv) = unbounded();
        // Drone 13
        let (d13_send, d13_recv) = unbounded();
        // Drone 14
        let (d14_send, d14_recv) = unbounded();
        // SC - needed to not make the drone crash
        let (d11_tx_cmd, d11_rx_cmd) = unbounded();
        let (d12_tx_cmd, d12_rx_cmd) = unbounded();
        let (d13_tx_cmd, d13_rx_cmd) = unbounded();
        let (d14_tx_cmd, d14_rx_cmd) = unbounded();

        // Drone 11
        let neighbours11 = HashMap::from([(12, d12_send.clone()), (1, c_send.clone()), (14, d14_send.clone())]);
        let mut drone = MyDrone::new(
            11,
            unbounded().0,
            d11_rx_cmd.clone(),
            d_recv.clone(),
            neighbours11,
            0.0,
        );
        // Drone 12
        let neighbours12 = HashMap::from([(11, d_send.clone()), (13, d13_send.clone())]);
        let mut drone2 = MyDrone::new(
            12,
            unbounded().0,
            d12_rx_cmd.clone(),
            d12_recv.clone(),
            neighbours12,
            0.0,
        );
        // Drone 13
        let neighbours13 = HashMap::from([(12, d12_send.clone()), (14, d14_send.clone())]);
        let mut drone3 = MyDrone::new(
            13,
            unbounded().0,
            d13_rx_cmd.clone(),
            d13_recv.clone(),
            neighbours13,
            0.0,
        );
        // Drone 14
        let neighbours14 = HashMap::from([(11, d_send.clone()), (13, d13_send.clone())]);
        let mut drone4 = MyDrone::new(
            14,
            unbounded().0,
            d14_rx_cmd.clone(),
            d14_recv.clone(),
            neighbours14,
            0.0,
        );

        // Spawn the drone's run method in a separate thread
        thread::spawn(move || {
            drone.run();
        });

        thread::spawn(move || {
            drone2.run();
        });

        thread::spawn(move || {
            drone3.run();
        });

        thread::spawn(move || {
            drone4.run();
        });

        let mut msg = Packet::new_flood_request(
            SourceRoutingHeader::new(vec![], 15),
            23,
            FloodRequest::initialize(56, 1, NodeType::Client),
        );

        d11_tx_cmd.send(DroneCommand::RemoveSender(14));
        d13_tx_cmd.send(DroneCommand::RemoveSender(14));
        d14_tx_cmd.send(DroneCommand::Crash);

        // "Client" sends packet to the drone
        d_send.send(msg.clone()).unwrap();

        assert_eq!(
            c_recv.recv().unwrap(),
            Packet {
                pack_type: PacketType::FloodResponse(FloodResponse { flood_id: 56, path_trace: vec![(1, NodeType::Client), (11, NodeType::Drone), (12, NodeType::Drone), (13, NodeType::Drone)] },),
                routing_header: SourceRoutingHeader {
                    hop_index: 3,
                    hops: vec![13, 12, 11, 1],
                },
                session_id: 23,
            }
        );
    }



}
fn main() {}

/*
struct SimulationController {
    drones: HashMap<NodeId, Sender<DroneCommand>>,
    node_event_recv: Receiver<DroneEvent>,
}

impl SimulationController {
    fn crash_all(&mut self) {
        for (_, sender) in self.drones.iter() {
            sender.send(DroneCommand::Crash).unwrap();
        }
    }
}

fn parse_config(file: &str) -> Config {
    let file_str = fs::read_to_string(file).unwrap();
    toml::from_str(&file_str).unwrap()
}

fn main() {
    let config = parse_config("./config.toml");

    let mut controller_drones = HashMap::new();
    let (node_event_send, node_event_recv) = unbounded();

    let mut packet_channels = HashMap::new();
    for drone in config.drone.iter() {
        packet_channels.insert(drone.id, unbounded());
    }
    for client in config.client.iter() {
        packet_channels.insert(client.id, unbounded());
    }
    for server in config.server.iter() {
        packet_channels.insert(server.id, unbounded());
    }

    let mut handles = Vec::new();
    for drone in config.drone.into_iter() {
        // controller
        let (controller_drone_send, controller_drone_recv) = unbounded();
        controller_drones.insert(drone.id, controller_drone_send);
        let node_event_send = node_event_send.clone();
        // packet
        let packet_recv = packet_channels[&drone.id].1.clone();
        let packet_send = drone
            .connected_node_ids
            .into_iter()
            .map(|id| (id, packet_channels[&id].0.clone()))
            .collect();

        handles.push(thread::spawn(move || {
            let mut drone = MyDrone::new(
                drone.id,
                node_event_send,
                controller_drone_recv,
                packet_recv,
                packet_send,
                drone.pdr,
            );

            drone.run();
        }));
    }
    let mut controller = SimulationController {
        drones: controller_drones,
        node_event_recv,
    };
    controller.crash_all();

    while let Some(handle) = handles.pop() {
        handle.join().unwrap();
    }
}
 */
