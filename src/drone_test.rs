

#[cfg(test)]
mod drone_tests {
    use wg_2024::{config::Client, tests};

    use crate::*;

    #[test]
    pub fn test_chain_fragment_ack() {
        wg_2024::tests::generic_chain_fragment_ack::<MyDrone>();
    }

    #[test]
    pub fn test_chain_fragment_drop() {
        wg_2024::tests::generic_chain_fragment_drop::<MyDrone>();
    }

    #[test]
    fn test_fragment_drop() {
        wg_2024::tests::generic_fragment_drop::<MyDrone>();
    }

    #[test]
    fn test_fragment_forward() {
        wg_2024::tests::generic_fragment_forward::<MyDrone>();
    }

    #[test]
    fn test_flooding_simple_topology_with_initiator_in_path_trace() {
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
                    println!("CLIENT RECEIVED: {:?}", f.path_trace);
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
    fn test_flooding_simple_topology_without_initiator_in_path_trace() {
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
            FloodRequest {
                flood_id: 56,
                initiator_id: 1,
                path_trace: vec![],
            },
        );

        // "Client" sends packet to the drone
        d_send.send(msg.clone()).unwrap();

        for _ in 0..3 {
            match c_recv.recv().unwrap().pack_type {
                PacketType::FloodResponse(f) => {
                    println!("CLIENT RECEIVED: {:?}", f.path_trace);
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
    fn test_set_pdr() {
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
    fn test_crash() {
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
        let neighbours11 = HashMap::from([
            (12, d12_send.clone()),
            (1, c_send.clone()),
            (14, d14_send.clone()),
        ]);
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
                pack_type: PacketType::FloodResponse(FloodResponse {
                    flood_id: 56,
                    path_trace: vec![
                        (1, NodeType::Client),
                        (11, NodeType::Drone),
                        (12, NodeType::Drone),
                        (13, NodeType::Drone)
                    ]
                },),
                routing_header: SourceRoutingHeader {
                    hop_index: 3,
                    hops: vec![13, 12, 11, 1],
                },
                session_id: 23,
            }
        );
    }
}
