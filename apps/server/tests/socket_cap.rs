use fraglink_server::presence::Hub;
use std::sync::Arc;

#[test]
fn socket_slots_are_capped_and_released() {
    let hub = Arc::new(Hub::default());
    let a = hub.try_reserve(2).expect("slot 1");
    let b = hub.try_reserve(2).expect("slot 2");
    assert!(hub.try_reserve(2).is_none(), "third socket must be refused");
    assert_eq!(hub.live_sockets(), 2);
    drop(a);
    let c = hub.try_reserve(2).expect("slot freed");
    drop((b, c));
    assert_eq!(hub.live_sockets(), 0);
}
