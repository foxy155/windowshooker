use crate::core::logging::format_now;
use crate::theme;

#[derive(Clone, Copy, PartialEq)]
pub enum Direction {
    Sent,
    Received,
}

#[derive(Clone)]
pub struct Packet {
    pub id: usize,
    pub source_pid: u32,
    pub direction: Direction,
    pub timestamp: String,
    pub size: usize,
    pub opcode: Option<u16>,
    pub data: Vec<u8>,
}

impl Packet {
    pub fn direction_label(&self) -> &'static str {
        match self.direction {
            Direction::Sent => "SEND",
            Direction::Received => "RECV",
        }
    }

    pub fn direction_color(&self) -> egui::Color32 {
        match self.direction {
            Direction::Sent => theme::sent(),
            Direction::Received => theme::recv(),
        }
    }

    pub fn hex_dump(&self) -> String {
        let mut out = String::new();
        for (i, chunk) in self.data.chunks(16).enumerate() {
            out.push_str(&format!("{:04x}  ", i * 16));
            for (j, byte) in chunk.iter().enumerate() {
                out.push_str(&format!("{:02x} ", byte));
                if j == 7 {
                    out.push(' ');
                }
            }
            for j in chunk.len()..16 {
                out.push_str("   ");
                if j == 7 {
                    out.push(' ');
                }
            }
            out.push_str(" |");
            for byte in chunk {
                let c = if byte.is_ascii_graphic() || *byte == b' ' {
                    *byte as char
                } else {
                    '.'
                };
                out.push(c);
            }
            out.push_str("|\n");
        }
        out
    }
}

pub struct PacketStore {
    pub packets: Vec<Packet>,
    next_id: usize,
    max_packets: usize,
}

impl PacketStore {
    pub fn new() -> Self {
        Self {
            packets: Vec::new(),
            next_id: 0,
            max_packets: 10000,
        }
    }

    pub fn push(&mut self, source_pid: u32, direction: Direction, data: Vec<u8>) {
        let id = self.next_id;
        self.next_id += 1;

        let opcode = if data.len() >= 2 {
            Some(u16::from_le_bytes([data[0], data[1]]))
        } else {
            None
        };

        let timestamp = format_now();

        self.packets.push(Packet {
            id,
            source_pid,
            direction,
            timestamp,
            size: data.len(),
            opcode,
            data,
        });

        if self.packets.len() > self.max_packets {
            let excess = self.packets.len() - self.max_packets;
            self.packets.drain(0..excess);
        }
    }

    pub fn clear(&mut self) {
        self.packets.clear();
    }
}