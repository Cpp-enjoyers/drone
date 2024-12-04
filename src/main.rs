#![allow(unused)]

use crossbeam_channel::{select_biased, unbounded, Receiver, Sender};
use log::{error, info, LevelFilter};
use rand::distributions::Bernoulli;
use rand::prelude::*;
use std::collections::HashMap;
use std::{fs, thread};
use wg_2024::config::Config;
use wg_2024::controller::{DroneCommand, DroneEvent};
use wg_2024::drone::Drone;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::*;

/// Example of drone implementation
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
        if cfg!(debug_assertions) {
            log::set_max_level(LevelFilter::Trace);
        } else {
            log::set_max_level(LevelFilter::Error);
        }
        let mut pdr_distribution = Bernoulli::new(0.0).unwrap();
        if let Ok(distr_from_settings) = Bernoulli::new(pdr as f64) {
            pdr_distribution = distr_from_settings;
        } else {
            error!("Error during drone creation: PDR invalid, setting to 0%.")
        }
        Self {
            id,
            controller_send,
            controller_recv,
            packet_recv,
            packet_send,
            pdr_distribution,
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
                default => {channels_clear = true;},
            }
        }
    }
}

impl MyDrone {
    fn send_nack(&self, packet: Packet, nack_type: NackType) {
        let mut reversed_routing_header =
            packet.routing_header.hops[..=packet.routing_header.hop_index].to_vec();
        reversed_routing_header.reverse();

        let nack = Packet {
            pack_type: PacketType::Nack(Nack {
                fragment_index: 0,
                nack_type,
            }),
            routing_header: SourceRoutingHeader {
                hop_index: 1,
                hops: reversed_routing_header.clone(),
            },
            session_id: packet.session_id,
        };
        // if packet type is Ack,Nack,FloodResponse, send original packet to sim controller
        // else send back nack
        match packet.pack_type {
            // send original packet to simulation controller
            PacketType::Nack(_) | PacketType::Ack(_) | PacketType::FloodResponse(_) => {
                self.controller_send
                    .send(DroneEvent::ControllerShortcut(packet));
            }

            // Send nack back
            PacketType::FloodRequest(_) | PacketType::MsgFragment(_) => {
                if (reversed_routing_header.len() < 2) {
                    info!("Too small routing header, aborting send.");
                    return;
                }
                if let Some(sender) = self
                    .packet_send
                    .get(reversed_routing_header.get(1).unwrap())
                {
                    self.controller_send
                        .send(DroneEvent::PacketSent(nack.clone()));
                    sender.send(nack);
                }
            }
        }
    }

    fn handle_packet(&mut self, packet: Packet) {
        // step1 and step3
        let hop_index: usize = packet.routing_header.hop_index;
        if hop_index + 1 >= packet.routing_header.hops.len() {
            Self::send_nack(self, packet, NackType::DestinationIsDrone);
            return;
        }
        let opt: Option<&u8> = packet.routing_header.hops.get(hop_index);
        if opt.is_none_or(|v: &u8| *v != self.id) {
            Self::send_nack(self, packet, NackType::UnexpectedRecipient(self.id));
            return;
        }
        // STEP 2
        let next_hop_index: usize = hop_index + 1;
        // STEP 4
        let next_hop_drone_id: u8 = packet.routing_header.hops[next_hop_index];
        if let Some(next_hop_drone_channel) = self.packet_send.get(&next_hop_drone_id) {
            self.handle_packet_internal(packet, next_hop_drone_channel, next_hop_index);
        } else {
            Self::send_nack(self, packet, NackType::ErrorInRouting(next_hop_drone_id));
        }
    }

    fn handle_packet_internal(
        &self,
        mut packet: Packet,
        send_to: &Sender<Packet>,
        next_hop_index: usize,
    ) {
        match packet.pack_type {
            PacketType::Nack(_) | PacketType::Ack(_) => {
                packet.routing_header.hop_index = next_hop_index;
                self.controller_send
                    .send(DroneEvent::PacketSent(packet.clone()));
                send_to.send(packet);
            }
            PacketType::MsgFragment(_) => {
                packet.routing_header.hop_index = next_hop_index;
                if self.pdr_distribution.sample(&mut rand::thread_rng()) {
                    self.controller_send
                        .send(DroneEvent::PacketDropped(packet.clone()));
                    self.send_nack(packet, NackType::Dropped);
                } else {
                    self.controller_send
                        .send(DroneEvent::PacketSent(packet.clone()));
                    send_to.send(packet);
                }
            }
            PacketType::FloodRequest(flood_request) => todo!(),
            PacketType::FloodResponse(flood_response) => todo!(),
        }
    }

    fn handle_command(&mut self, command: DroneCommand) {
        match command {
            DroneCommand::AddSender(node_id, sender) => {
                self.packet_send.insert(node_id, sender);
            }
            DroneCommand::SetPacketDropRate(pdr) => {
                if let Ok(new_pdr) = Bernoulli::new(pdr as f64) {
                    self.pdr_distribution = new_pdr;
                } else {
                    info!("PDR set by sim contr is not valid, keeping previous one");
                }
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
