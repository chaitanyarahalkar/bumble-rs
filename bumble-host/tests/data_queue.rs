use bumble_host::{DataPacketQueue, DataPacketQueueError};

#[test]
fn enforces_global_window_and_releases_fifo_on_completion() {
    let mut queue = DataPacketQueue::new(2).unwrap();
    queue.enqueue("a1", 1);
    queue.enqueue("b1", 2);
    queue.enqueue("a2", 1);
    assert_eq!(queue.queued(), 3);
    assert_eq!(queue.pending(), 3);

    assert_eq!(queue.poll_ready(), Some("a1"));
    assert_eq!(queue.poll_ready(), Some("b1"));
    assert_eq!(queue.poll_ready(), None);
    assert_eq!(queue.in_flight(), 2);
    assert_eq!(queue.connection_in_flight(1), 1);
    assert_eq!(queue.waiting(), 1);

    queue.on_packets_completed(1, 1).unwrap();
    assert_eq!(queue.completed(), 1);
    assert_eq!(queue.poll_ready(), Some("a2"));
    assert_eq!(queue.connection_in_flight(1), 1);
    assert!(!queue.is_drained(1));
    queue.on_packets_completed(1, 2).unwrap();
    queue.on_packets_completed(1, 1).unwrap();
    assert_eq!(queue.completed(), 3);
    assert_eq!(queue.pending(), 0);
    assert!(queue.is_drained(1));
    assert!(queue.is_drained(2));
}

#[test]
fn completion_errors_are_bounded_and_flush_accounts_for_all_packets() {
    assert_eq!(
        DataPacketQueue::<u8>::new(0).unwrap_err(),
        DataPacketQueueError::ZeroCapacity
    );
    let mut queue = DataPacketQueue::new(2).unwrap();
    queue.enqueue(1, 7);
    queue.enqueue(2, 7);
    queue.enqueue(3, 8);
    assert_eq!(queue.poll_ready(), Some(1));
    assert_eq!(queue.poll_ready(), Some(2));
    assert_eq!(
        queue.on_packets_completed(3, 7).unwrap_err(),
        DataPacketQueueError::CompletionOverflow {
            connection_handle: 7,
            completed: 3,
            in_flight: 2,
        }
    );
    assert_eq!(queue.in_flight(), 0);
    assert_eq!(queue.completed(), 2);
    assert_eq!(queue.flush(8), 1);
    assert_eq!(queue.completed(), 3);
    assert_eq!(queue.pending(), 0);
    assert_eq!(
        queue.on_packets_completed(1, 99).unwrap_err(),
        DataPacketQueueError::UnknownConnection(99)
    );
}
