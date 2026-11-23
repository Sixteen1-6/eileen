//! The bus behind the driver: `Emit` effects reach its subscribers through the run, and a
//! subscriber that never reads holds neither the core nor the other subscribers.

use eileen_contracts::bus::BusEvent;
use eileen_contracts::core_io::Effect;
use eileen_contracts::ids::{Envelope, TraceCtx};
use tokio::time::timeout;

use super::{Limits, PATIENCE, Probes, Shutdown, envelope, nz, scripted, steps_of, toggle};
use crate::bus;

#[tokio::test(start_paused = true)]
async fn a_subscriber_that_never_reads_never_holds_the_core() {
    const INPUTS: u64 = 50;
    let (probes, _seen) = Probes::new();
    let (feeder, sink) = probes.feeder("feeder", &[]);
    let (bus, handle) = bus::bounded(nz(4));
    // A component queue this small fills at once if the bus ever waits for a subscriber.
    let limits = Limits { effects: nz(2), ..Limits::default() };
    let (driver, steps) = scripted(limits, |_| {
        vec![
            Effect::Emit(BusEvent::MuteChanged { muted: true }),
            Effect::Emit(BusEvent::MuteChanged { muted: false }),
        ]
    });
    let mut stalled = handle.subscribe();
    let mut reader = handle.subscribe();
    let shutdown = Shutdown::new();
    let run = eileen_rt::task::spawn(
        "test.driver",
        driver.run(vec![feeder, Box::new(bus)], shutdown.clone()),
    );
    let sink = sink.await.unwrap();

    for n in 1..=INPUTS {
        sink.send(envelope(toggle())).unwrap();
        for (seq, muted) in [(2 * n - 1, true), (2 * n, false)] {
            let event = timeout(PATIENCE, reader.recv()).await.expect("the reader was held");
            let Envelope { seq: got, ctx, body, .. } = event.expect("the bus is running");
            assert_eq!((got, body), (seq, BusEvent::MuteChanged { muted }), "the core's event");
            assert_eq!(ctx, TraceCtx::default(), "with the context the core gave it");
        }
    }
    assert_eq!(steps_of(&steps).len(), 50, "the core took every input");

    let notice = timeout(PATIENCE, stalled.recv()).await.unwrap().expect("the bus is running");
    assert_eq!(
        (notice.seq, notice.body),
        (2 * INPUTS - 3, BusEvent::Lagged { missed: 2 * INPUTS - 4 }),
        "the stalled subscriber is told what it missed"
    );
    for seq in 2 * INPUTS - 3..=2 * INPUTS {
        let event = timeout(PATIENCE, stalled.recv()).await.unwrap().expect("a kept event");
        assert_eq!(event.seq, seq, "then the four events still kept");
    }

    shutdown.trigger();
    assert_eq!(run.await.unwrap(), Ok(()), "the run ends well");
    assert_eq!(stalled.recv().await, None, "and the bus has stopped with it");
}
