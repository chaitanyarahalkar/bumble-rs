use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataPacketQueueError {
    ZeroCapacity,
    UnknownConnection(u16),
    CompletionOverflow {
        connection_handle: u16,
        completed: usize,
        in_flight: usize,
    },
}

impl core::fmt::Display for DataPacketQueueError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ZeroCapacity => f.write_str("packet queue capacity must be nonzero"),
            Self::UnknownConnection(handle) => {
                write!(f, "completion for unknown connection 0x{handle:04X}")
            }
            Self::CompletionOverflow {
                connection_handle,
                completed,
                in_flight,
            } => write!(
                f,
                "{completed} packets completed for 0x{connection_handle:04X}, but only {in_flight} are in flight"
            ),
        }
    }
}

impl std::error::Error for DataPacketQueueError {}

/// Sans-I/O host-to-controller packet flow-control queue.
#[derive(Clone, Debug)]
pub struct DataPacketQueue<T> {
    max_in_flight: usize,
    in_flight: usize,
    per_connection_in_flight: BTreeMap<u16, usize>,
    packets: VecDeque<(T, u16)>,
    queued: usize,
    completed: usize,
}

impl<T> DataPacketQueue<T> {
    pub fn new(max_in_flight: usize) -> Result<Self, DataPacketQueueError> {
        if max_in_flight == 0 {
            return Err(DataPacketQueueError::ZeroCapacity);
        }
        Ok(Self {
            max_in_flight,
            in_flight: 0,
            per_connection_in_flight: BTreeMap::new(),
            packets: VecDeque::new(),
            queued: 0,
            completed: 0,
        })
    }

    pub fn enqueue(&mut self, packet: T, connection_handle: u16) {
        self.packets.push_back((packet, connection_handle));
        self.queued += 1;
    }

    /// Return the next packet the controller has room to accept and mark it in
    /// flight. Returns `None` while the controller window is full.
    pub fn poll_ready(&mut self) -> Option<T> {
        if self.in_flight >= self.max_in_flight {
            return None;
        }
        let (packet, handle) = self.packets.pop_front()?;
        self.in_flight += 1;
        *self.per_connection_in_flight.entry(handle).or_default() += 1;
        Some(packet)
    }

    pub fn on_packets_completed(
        &mut self,
        packet_count: usize,
        connection_handle: u16,
    ) -> Result<(), DataPacketQueueError> {
        let Some(connection_in_flight) = self.per_connection_in_flight.get_mut(&connection_handle)
        else {
            return Err(DataPacketQueueError::UnknownConnection(connection_handle));
        };
        if packet_count > *connection_in_flight {
            let in_flight = *connection_in_flight;
            self.in_flight -= in_flight;
            self.completed += in_flight;
            *connection_in_flight = 0;
            return Err(DataPacketQueueError::CompletionOverflow {
                connection_handle,
                completed: packet_count,
                in_flight,
            });
        }
        *connection_in_flight -= packet_count;
        self.in_flight -= packet_count;
        self.completed += packet_count;
        Ok(())
    }

    /// Drop queued packets and implicitly complete in-flight packets for one
    /// disconnected handle.
    pub fn flush(&mut self, connection_handle: u16) -> usize {
        let before = self.packets.len();
        self.packets
            .retain(|(_, handle)| *handle != connection_handle);
        let queued = before - self.packets.len();
        let in_flight = self
            .per_connection_in_flight
            .remove(&connection_handle)
            .unwrap_or(0);
        self.in_flight -= in_flight;
        self.completed += queued + in_flight;
        queued + in_flight
    }

    /// Drop every queued packet and implicitly complete all in-flight packets.
    ///
    /// This is the transport-wide counterpart to [`Self::flush`] used when the
    /// HCI host is flushed or reset and no connection handle remains valid.
    pub fn flush_all(&mut self) -> usize {
        let pending = self.packets.len() + self.in_flight;
        self.packets.clear();
        self.per_connection_in_flight.clear();
        self.in_flight = 0;
        self.completed += pending;
        pending
    }

    pub fn queued(&self) -> usize {
        self.queued
    }

    pub fn completed(&self) -> usize {
        self.completed
    }

    pub fn pending(&self) -> usize {
        self.queued - self.completed
    }

    pub fn waiting(&self) -> usize {
        self.packets.len()
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight
    }

    pub fn max_in_flight(&self) -> usize {
        self.max_in_flight
    }

    pub fn connection_in_flight(&self, connection_handle: u16) -> usize {
        self.per_connection_in_flight
            .get(&connection_handle)
            .copied()
            .unwrap_or(0)
    }

    pub fn is_drained(&self, connection_handle: u16) -> bool {
        self.connection_in_flight(connection_handle) == 0
            && !self
                .packets
                .iter()
                .any(|(_, handle)| *handle == connection_handle)
    }
}
