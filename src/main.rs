#![allow(unused)]

use crossbeam_channel::{select_biased, unbounded, Receiver, Sender};
use log::{error, info, warn, LevelFilter};
use rand::distributions::Bernoulli;
use rand::prelude::*;
use std::collections::HashMap;
use std::{fs, thread};
use wg_2024::config::Config;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::*;

// TODO handle c.send() errors, just log the error I guess

struct MyDrone {
    id: NodeId,
    controller_send: Sender<DroneEvent>,
    controller_recv: Receiver<DroneCommand>,
    packet_recv: Receiver<Packet>,
    pdr_distribution: Bernoulli,
    packet_send: HashMap<NodeId, Sender<Packet>>,
    shutdown_received: bool,
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
        pretty_env_logger::try_init();
        log::set_max_level(if cfg!(debug_assertions) {
            LevelFilter::Trace
        } else {
            LevelFilter::Error
        });

        let mut pdr_distribution =
            Bernoulli::new(pdr as f64).unwrap_or_else(|e: rand::distributions::BernoulliError| {
                warn!("Invalide pdr ({pdr}) requested, defaulting to 0%");
                Bernoulli::new(0.).unwrap()
            });

        Self {
            id,
            controller_send,
            controller_recv,
            packet_recv,
            packet_send,
            pdr_distribution,
            // TODO how to remove?
            shutdown_received: false,
        }
    }

    fn run(&mut self) {
        let mut channels_clear = false;
        while !channels_clear && !self.shutdown_received {
            channels_clear = false;
            select_biased! {
                recv(self.controller_recv) -> command => {
                    if let Ok(command) = command {
                        self.handle_command(command);
                    }
                },
                recv(self.packet_recv) -> packet => {
                    if let Ok(packet) = packet {
                        self.handle_packet(packet);
                    }
                },
                // TODO check if this does what you think
                default => { channels_clear = true; },
            }
        }
    }
}

impl MyDrone {
    fn send_nack(&self, packet: Packet, nack_type: NackType) {
        let mut source_header: SourceRoutingHeader = packet
            .routing_header
            .sub_route(packet.routing_header.hop_index - 1..=0)
            .expect("this should not be happening");
        source_header.increase_hop_index();

        // if packet type is Ack,Nack,FloodResponse, send original packet to sim controller
        // else send back nack
        match packet.pack_type {
            // send original packet to simulation controller
            PacketType::Nack(_) | PacketType::Ack(_) | PacketType::FloodResponse(_) => {
                self.controller_send
                    .send(DroneEvent::ControllerShortcut(packet));
            }
            // Send nack back
            PacketType::MsgFragment(_) => {
                if (source_header.hops.len() < 2) {
                    error!("Too small routing header, aborting send.");
                    return;
                }
                // ? is this better?
                self.packet_send
                    .get(&source_header.current_hop().unwrap())
                    .map_or_else(
                        || info!("what to say here?"),
                        |s: &Sender<Packet>| {
                            let nack: Packet = Packet::new_nack(
                                source_header,
                                packet.session_id,
                                Nack {
                                    fragment_index: 0,
                                    nack_type,
                                },
                            );
                            self.controller_send
                                .send(DroneEvent::PacketSent(nack.clone()));
                            s.send(nack);
                        },
                    )
            }
            PacketType::FloodRequest(_) => {} // TODO put unreachable if needed
        }
    }

    fn handle_packet(&mut self, mut packet: Packet) {
        if let PacketType::FloodRequest(fr) = packet.pack_type {
            let Packet { routing_header, session_id, ..} = packet;
            self.handle_flood(routing_header, session_id, fr);
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
            self.send_nack(packet, NackType::UnexpectedRecipient(self.id));
            return;
        }
        let next_hop: Option<NodeId> = packet.routing_header.current_hop();
        if next_hop.is_none() {
            self.send_nack(packet, NackType::DestinationIsDrone);
            return;
        }

        // STEP 4
        let next_hop_drone_id: NodeId = next_hop.expect("this should not be happening");
        let next_hop_channel: Option<&Sender<Packet>> = self.packet_send.get(&next_hop_drone_id);
        if next_hop_channel.is_none() {
            self.send_nack(packet, NackType::ErrorInRouting(next_hop_drone_id));
            return;
        }

        self.send_packet(
            packet,
            next_hop_channel.expect("this should not be happening"),
        );
    }

    fn send_packet(&self, packet: Packet, channel: &Sender<Packet>) {
        match packet.pack_type {
            PacketType::Nack(_) | PacketType::Ack(_) => {
                // packet.routing_header.hop_index = next_hop_index;
                self.controller_send
                    .send(DroneEvent::PacketSent(packet.clone()));
                channel.send(packet);
            }
            PacketType::MsgFragment(_) => {
                // packet.routing_header.hop_index = next_hop_index;
                if self.pdr_distribution.sample(&mut rand::thread_rng()) {
                    self.controller_send
                        .send(DroneEvent::PacketDropped(packet.clone()));
                    self.send_nack(packet, NackType::Dropped);
                } else {
                    self.controller_send
                        .send(DroneEvent::PacketSent(packet.clone()));
                    channel.send(packet);
                }
            }
            PacketType::FloodResponse(_) => {
                // TODO check if this is the right thing to do
                self.controller_send
                    .send(DroneEvent::PacketSent(packet.clone()));
                channel.send(packet);
            }
            PacketType::FloodRequest(_) => unreachable!(),
        }
    }

    fn handle_flood(&self, routing_header: SourceRoutingHeader, sid: u64, mut flood_r: FloodRequest) {
        let sender_tuple = flood_r.path_trace.last();
        if sender_tuple.is_none() {
            error!("Received a flood request with empty path_trace!!!");
            return;
        }

        let &(sender_id, _) = sender_tuple.unwrap();
        flood_r.increment(self.id, NodeType::Drone);

        // ! TODO id is here?? 
        // TODO this should handle both floodResponse cases (check on hashmap size)
        if (true /* TODO */) {
            let mut new_packet: Packet = flood_r.generate_response(sid);
            new_packet.routing_header.increase_hop_index();
            let next_hop: Option<NodeId> = new_packet.routing_header.current_hop();
            self.packet_send.get(&next_hop.unwrap()).map_or_else(
                || info!("what to say here?"), 
                |c: &Sender<Packet>| self.send_packet(new_packet, c)
            );
            return;
        } 
        // TODO add the check if the neighbour exists in a debug only log (#[...] { error!(...); })
        self.packet_send.iter().for_each(|(id, c)| {
            if *id == sender_id {
                return;
            }
            c.send(Packet::new_flood_request(routing_header.clone(), sid, flood_r.clone()));
        });
    }

    fn handle_command(&mut self, command: DroneCommand) {
        match command {
            // TODO why is only packet drop rate logging??
            DroneCommand::AddSender(node_id, sender) => {
                self.packet_send.insert(node_id, sender);
            }
            DroneCommand::SetPacketDropRate(pdr) => {
                if let Ok(new_pdr) = Bernoulli::new(pdr as f64) {
                    self.pdr_distribution = new_pdr;
                    return;
                }
                warn!("PDR set by sim contr is not valid, keeping previous one");
            }
            DroneCommand::Crash => self.shutdown_received = true,
            DroneCommand::RemoveSender(node_id) => {
                self.packet_send.remove(&node_id);
            }
        }
    }
}

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
