#![allow(unused)]

use crossbeam_channel::{select_biased, unbounded, Receiver, Sender};
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
    pdr: f32,
    packet_send: HashMap<NodeId, Sender<Packet>>,
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
        Self {
            id,
            controller_send,
            controller_recv,
            packet_recv,
            packet_send,
            pdr,
        }
    }

    fn run(&mut self) {
        loop {
            select_biased! {
                recv(self.controller_recv) -> command => {
                    if let Ok(command) = command {
                        if let DroneCommand::Crash = command {
                            println!("drone {} crashed", self.id);
                            break;
                        }
                        self.handle_command(command);
                    }
                }
                recv(self.packet_recv) -> packet => {
                    if let Ok(packet) = packet {
                        self.handle_packet(packet);
                    }
                },
            }
        }
    }
}

impl MyDrone {
    fn create_nack() {
        /*
         let mut reversed_routing_header = packet.routing_header.hops[..=packet.routing_header.hop_index].to_vec();
        reversed_routing_header.reverse();

        let nack = Packet{
            pack_type: PacketType::Nack(Nack {
                fragment_index: 0,
                nack_type: NackType::UnexpectedRecipient(self.id.clone())
            }),
            routing_header: SourceRoutingHeader{
                hop_index: 1,
                hops: reversed_routing_header.clone(),
            },
            session_id: packet.session_id
        };
        // send if needed or use another fun
         */
        todo!();
    }

    fn handle_packet(&mut self, packet: Packet) {
        /* let (f1, f2) =  */ match packet.pack_type {
            PacketType::Nack(_nack) => todo!(),
            PacketType::Ack(_ack) => todo!(),
            PacketType::MsgFragment(_fragment) => todo!(),
            PacketType::FloodRequest(_flood_request) => todo!(),
            PacketType::FloodResponse(_flood_response) => todo!(),
        }

        // step1 and step3
        let hop_index: usize = packet.routing_header.hop_index;
        if hop_index + 1 >= packet.routing_header.hops.len() {
            // check if Nack::new() exists
            Self::create_nack();
        }
        let opt: Option<&u8> = packet.routing_header.hops.get(hop_index);
        if opt.is_none_or(|v: &u8| *v != self.id) {
            Self::create_nack();
        }

        // STEP 2
        let next_hop_index: usize = hop_index + 1;

        // STEP 4
        let next_hop_drone_id: u8 = packet.routing_header.hops[next_hop_index];
        let next_hop_drone_channel: Option<&Sender<Packet>> = self.packet_send.get(&next_hop_drone_id);
        if next_hop_drone_channel.is_none() {
            Self::create_nack();
        }

        // f2()
    }

    fn handle_command(&mut self, command: DroneCommand) {
        match command {
            DroneCommand::AddSender(_node_id, _sender) => self.add_sender(_node_id, _sender),
            DroneCommand::SetPacketDropRate(_pdr) => self.set_packet_drop_rate(_pdr),
            DroneCommand::Crash => unreachable!(),
            DroneCommand::RemoveSender(_node_id) => self.remove_sender(_node_id),
        }
    }

    fn add_sender(&mut self, _node_id: NodeId, _sender: Sender<Packet>){
        // check?
        self.packet_send.insert(_node_id, _sender);
    }

    fn set_packet_drop_rate(&mut self, _pdr: f32){
        // check?
        self.pdr = _pdr;
    }

    fn remove_sender(&mut self, _node_id: NodeId){
        // check?
        self.packet_send.remove(&_node_id);
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