#![allow(unused)]

use crossbeam_channel::Sender;
use wg_2024::network::{NodeId, SourceRoutingHeader};
use wg_2024::packet::*;

// use super::utils;

/*
 * TODO: enum for msg types with a generic trait msg,
 *       Client/Server trait (force it to use Flooder trait)
 *       utils library (if needed)
 */

// ! NOTE we can have the function take a packet but then we would need another enum match
pub trait Flooder {
    // associated constants (looks like a good idea)
    const NODE_TYPE: NodeType;

    fn get_id(&self) -> u8;
    fn get_neighbours(&self) -> impl ExactSizeIterator<Item = &(NodeId, Sender<Packet>)>;
    fn has_seen_flood(&self, flood_id: (u8, u64)) -> bool;
    fn insert_flood(&mut self, flood_id: (u8, u64));
    fn send_to_controller(&self, p: Packet);

    //
    fn handle_flood_request(
        &mut self,
        routing_header: SourceRoutingHeader,
        sid: u64,
        mut flood_r: FloodRequest,
    ) -> Result<(), ()> {
        let sender_id: u8 = flood_r
            .path_trace
            .last()
            .map_or(flood_r.initiator_id, |(id, t)| *id);
        let flood_tuple_id = (flood_r.initiator_id, flood_r.flood_id);

        flood_r.increment(self.get_id(), Self::NODE_TYPE);

        let mut it = self.get_neighbours();
        if self.has_seen_flood(flood_tuple_id) || it.len() <= 1 {
            let mut new_packet: Packet = flood_r.generate_response(sid);
            new_packet.routing_header.increase_hop_index();
            let next_hop: NodeId = new_packet
                .routing_header
                .current_hop()
                .expect("If this panics the wg code is borken");
            match it.find(|(id, c)| *id == next_hop) {
                Some((_, c)) => {
                    c.send(new_packet.clone());
                    self.send_to_controller(new_packet);
                    Ok(())
                }
                None => Err(()),
            }
        } else {
            it.for_each(|(id, c)| {
                if *id != sender_id {
                    let new_packet =
                        Packet::new_flood_request(routing_header.clone(), sid, flood_r.clone());
                    c.send(new_packet.clone());
                    self.send_to_controller(new_packet);
                }
            });
            self.insert_flood(flood_tuple_id);
            Ok(())
        }
    }
}
