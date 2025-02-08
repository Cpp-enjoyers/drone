#![allow(unused)]

use crossbeam_channel::{select, select_biased, unbounded, Receiver, Sender};
use log::{error, info, trace, warn, LevelFilter};
use rand::distributions::Bernoulli;
use rand::prelude::*;
use std::collections::{HashMap, HashSet};
use std::{default, env, fs, thread};
use wg_2024::config::Config;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::{FloodRequest, Nack, NackType, NodeType, Packet, PacketType};
use wg_2024::tests::*;

const RING_BUFF_SZ: usize = 64;

#[cfg(feature = "unlimited_buffer")]
type RingBuffer<T> = HashSet<T>;
#[cfg(not(feature = "unlimited_buffer"))]
use common::ring_buffer::RingBuffer;

// internal state of the drone
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

/// C++Enyoers Drone struct
#[allow(clippy::module_name_repetitions)]
#[derive(Debug)]
pub struct CppEnjoyersDrone {
    // drone's id in the topology
    id: NodeId,
    // string log prefix
    log_channel: String,
    // channel to scl
    controller_send: Sender<DroneEvent>,
    // channel from scl
    controller_recv: Receiver<DroneCommand>,
    // channel to read incoming packets
    packet_recv: Receiver<Packet>,
    // Packet Drop Rate
    pdr_distribution: Bernoulli,
    // channel to talk with direct neighbors
    packet_send: HashMap<NodeId, Sender<Packet>>,
    // internal state if the drone
    state: State,
    // history of flood requests that the drone has received, divided by initiator
    flood_history: HashMap<NodeId, RingBuffer<u64>>,
}

impl Drone for CppEnjoyersDrone {
    /// Drone's constructor
    /// * id: donre's id in the network
    /// * `constroller_send`: channel to scl
    /// * `controller_recv`: channel from scl
    /// * `packet_recv`: channel to read incoming messages
    /// * `packet_send`: map that links a neighbor with its channel
    /// * pdr: Packet Drop Rate
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

        let log_channel: String = format!("CppEnjoyers[{id}]");
        let mut pdr_distribution: Bernoulli = Bernoulli::new(f64::from(pdr)).unwrap_or_else(
            |e: rand::distributions::BernoulliError| {
                warn!(target: &log_channel, "Invalid PDR {}, setting to 0.0", pdr);
                Bernoulli::new(0.).unwrap()
            },
        );
        Self {
            id,
            log_channel,
            controller_send,
            controller_recv,
            packet_recv,
            packet_send,
            pdr_distribution,
            state: State::Running,
            flood_history: HashMap::new(),
        }
    }

    /// Core function that starts the drone
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
        while let Ok(packet) = self.packet_recv.recv() {
            info!(target: &self.log_channel, "Processing packet during crash: {:?}", packet);
            self.handle_crash(packet);
        }
        info!(target: &self.log_channel, "Crashing gently");
    }
}

impl CppEnjoyersDrone {
    // sends a nack of a given type related to the given packet
    fn send_nack(&self, mut packet: Packet, nack_type: NackType) {
        let mut source_header: SourceRoutingHeader = if let Some(sh) = packet
            .routing_header
            .sub_route(0..packet.routing_header.hop_index)
        {
            sh
        } else {
            error!(target: &self.log_channel, "\tCan't invert source routing header: {}. Dropping Nack packet", packet.routing_header);
            return;
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
                    );
            }
            PacketType::FloodRequest(_) => {
                error!(target: &self.log_channel, "\tFound a flood request while trying to send back Nack. You probably found a bug :(");
                unreachable!()
            }
        }
    }

    // handles a packet from another drone according to WG protocol
    fn handle_packet(&mut self, mut packet: Packet) {
        if let PacketType::FloodRequest(fr) = packet.pack_type {
            info!(target: &self.log_channel, "\tHandling flood request");
            let Packet {
                routing_header,
                session_id,
                ..
            } = packet;
            self.handle_flood_request(&routing_header, session_id, fr);
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
        if let Some(next_hop) = next_hop {
            let next_hop_drone_id: NodeId = next_hop;
            let next_hop_channel: Option<&Sender<Packet>> =
                self.packet_send.get(&next_hop_drone_id);

            if let Some(next_hop_channel) = next_hop_channel {
                self.send_packet(packet, next_hop_channel);
            } else {
                info!(target: &self.log_channel, "\tCreating Nack - Error in routing");
                self.send_nack(packet, NackType::ErrorInRouting(next_hop_drone_id));
            }
        } else {
            info!(target: & self.log_channel, "\tCreating Nack - Drone is destination");
            self.send_nack(packet, NackType::DestinationIsDrone);
        }
    }

    // sends the given packet over the given channel and logs to scl the PacketSent event
    fn send_packet(&self, packet: Packet, channel: &Sender<Packet>) {
        match packet.pack_type {
            PacketType::Nack(_) | PacketType::Ack(_) => {
                self.controller_send
                    .send(DroneEvent::PacketSent(packet.clone()));
                info!(target: &self.log_channel, "\tLogging and sending packet to {}", packet.routing_header.current_hop().expect("If we panic here there's a bug :("));
                channel.send(packet);
            }
            // toss a coin for dropping the fragment
            PacketType::MsgFragment(_) => {
                if self.pdr_distribution.sample(&mut rand::thread_rng()) {
                    let mut to_send: Packet = packet.clone();
                    to_send.routing_header.decrease_hop_index();
                    self.controller_send
                        .send(DroneEvent::PacketDropped(to_send));
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
                self.controller_send
                    .send(DroneEvent::PacketSent(packet.clone()));
                info!(target: &self.log_channel, "\tLogging and forwarding flood request: {}", packet);
                channel.send(packet);
            }
        }
    }

    // handles flood requests differently from other packet types
    fn handle_flood_request(
        &mut self,
        routing_header: &SourceRoutingHeader,
        sid: u64,
        mut flood_r: FloodRequest,
    ) {
        let sender_tuple: (NodeId, NodeType) = if let Some(tup) = flood_r.path_trace.last() {
            *tup
        } else {
            warn!(target: &self.log_channel, "\tReceived flood request with empty path trace. Assuming it arrived from the initiator");
            (flood_r.initiator_id, NodeType::Client) // assume it's a client, who cares
        };

        let (sender_id, _) = sender_tuple;
        flood_r.increment(self.id, NodeType::Drone);

        let ring_buff = self
            .flood_history
            .entry(flood_r.initiator_id)
            .or_insert_with(|| RingBuffer::with_capacity(RING_BUFF_SZ));

        if (ring_buff.contains(&flood_r.flood_id) || self.packet_send.len() <= 1) {
            let mut new_packet: Packet = flood_r.generate_response(sid);
            info!(target: &self.log_channel, "\tGenerated flood response: {}", new_packet);
            new_packet.routing_header.increase_hop_index();
            let next_hop: NodeId = new_packet
                .routing_header
                .current_hop()
                .expect("If this panics here the code is bugged");
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

        ring_buff.insert(flood_r.flood_id);
        // forward flood request
        self.packet_send.iter().for_each(|(id, c)| {
            if *id != sender_id {
                let new_packet =
                    Packet::new_flood_request(routing_header.clone(), sid, flood_r.clone());
                self.send_packet(new_packet, c);
            }
        });
    }

    // handles a command sent by the scl
    fn handle_command(&mut self, command: DroneCommand) {
        match command {
            DroneCommand::AddSender(node_id, sender) => {
                info!(target: &self.log_channel, "\tAdding sender channel to {}", node_id);
                self.packet_send.insert(node_id, sender);
            }
            DroneCommand::SetPacketDropRate(pdr) => {
                if let Ok(new_pdr) = Bernoulli::new(f64::from(pdr)) {
                    info!(target: &self.log_channel, "\tSetting new PDR: {}", pdr);
                    self.pdr_distribution = new_pdr;
                } else {
                    warn!(target: &self.log_channel, "\tInvalid PDR: {}, keeping old value", pdr);
                }
            }
            DroneCommand::Crash => {
                info!(target: &self.log_channel, "\tStarting crash routine");
                self.state = State::Crashing;
            }
            DroneCommand::RemoveSender(node_id) => {
                info!(target: &self.log_channel, "\tRemoving sender channel to id: {}", node_id);
                self.packet_send.remove(&node_id);
            }
        }
    }

    // handles packets in crashing mode according to WG protocol
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

    // tells if the drone is in running mode
    #[inline]
    fn is_running(&self) -> bool {
        self.state.is_running()
    }

    // tells if the drone is in crashing mode
    #[inline]
    fn is_crashing(&self) -> bool {
        self.state.is_crashing()
    }
}
