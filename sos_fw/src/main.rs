#![no_std]
#![no_main]

use core::cell::RefCell;

use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::main;
use esp_hal::uart::{Config as UartConfig, Uart};
use esp_hal::Blocking;

use heapless::Deque;
use heapless::Vec as HVec;

use sos_core::apps::{AnyApp, BatteryApp, HeartbeatApp, TempSensorApp};
use sos_core::{
    AuthToken, BusHandle, BusMessage, CommandOutcome, NackReason, Scheduler, WireMessage,
    WIRE_FRAME_CAP,
};

esp_bootloader_esp_idf::esp_app_desc!();

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    esp_println::println!("panic: {info}");
    loop {}
}

/// How many uplinked `Command`s can be in flight (sent to the bus, result
/// not yet reported back to ground) at once — the `no_std`-sized answer to
/// `sos_cli::run_sim`'s unbounded `VecDeque<u32>`.
const MAX_PENDING_COMMANDS: usize = 8;

/// The satellite side of the ground link `sos_cli::ground_control` speaks:
/// same `WireMessage` framing, same auth-then-sequence-numbered protocol,
/// just over a physical UART instead of TCP, and driven from one thread's
/// tick loop instead of a reader/writer thread pair.
struct UartLink<'d> {
    uart: Uart<'d, Blocking>,
    bus: Option<BusHandle>,
    authenticated: bool,
    downlink_seq: u32,
    last_uplink_seq: Option<u32>,
    pending_command_seqs: Deque<u32, MAX_PENDING_COMMANDS>,
    rx_buf: HVec<u8, WIRE_FRAME_CAP>,
}

impl<'d> UartLink<'d> {
    fn new(uart: Uart<'d, Blocking>) -> Self {
        UartLink {
            uart,
            bus: None,
            authenticated: false,
            downlink_seq: 0,
            last_uplink_seq: None,
            pending_command_seqs: Deque::new(),
            rx_buf: HVec::new(),
        }
    }

    fn send_wire(&mut self, msg: &WireMessage) {
        let Ok(frame) = msg.to_frame() else {
            return;
        };
        let mut data = frame.as_slice();
        while !data.is_empty() {
            match self.uart.write(data) {
                Ok(0) | Err(_) => break,
                Ok(n) => data = &data[n..],
            }
        }
        let _ = self.uart.flush();
    }

    fn send_bus_message(&mut self, msg: &BusMessage) {
        self.downlink_seq = self.downlink_seq.wrapping_add(1);
        let wire = WireMessage::Bus { seq: self.downlink_seq, msg: msg.clone() };
        self.send_wire(&wire);
    }

    fn send_command_outcome(&mut self, outcome: &CommandOutcome) {
        match self.pending_command_seqs.pop_front() {
            Some(seq) => self.send_wire(&WireMessage::CommandResult { seq, outcome: outcome.clone() }),
            None => esp_println::println!("uart-link: command outcome with no pending seq to pair it with"),
        }
    }

    /// Drains whatever bytes have arrived on the UART since the last call
    /// and processes every complete (0x00-terminated) frame found.
    ///
    /// Called once per tick from the `Heartbeat` branch of the scheduler
    /// sink, since `HeartbeatApp` fires exactly once per tick — the
    /// cheapest reliable "do this once a tick" hook `Scheduler::run`
    /// offers without changing its signature.
    fn poll_rx(&mut self) {
        let mut tmp = [0u8; 32];
        let n = match self.uart.read_buffered(&mut tmp) {
            Ok(n) => n,
            Err(_) => return,
        };
        for &byte in &tmp[..n] {
            if self.rx_buf.push(byte).is_err() {
                esp_println::println!("uart-link: frame exceeded WIRE_FRAME_CAP, resyncing");
                self.rx_buf.clear();
                continue;
            }
            if byte == 0 {
                self.handle_frame();
                self.rx_buf.clear();
            }
        }
    }

    fn handle_frame(&mut self) {
        let mut frame = self.rx_buf.clone();

        if !self.authenticated {
            match AuthToken::from_frame(&mut frame) {
                Ok(token) if sos_core::verify(&token.0) => {
                    self.authenticated = true;
                    esp_println::println!("uart-link: ground station authenticated");
                }
                _ => esp_println::println!("uart-link: rejected connection: bad auth token"),
            }
            return;
        }

        let Some(bus) = self.bus else { return };

        match WireMessage::from_frame(&mut frame) {
            Ok(WireMessage::Bus { seq, msg }) => {
                if self.last_uplink_seq.is_some_and(|last| seq <= last) {
                    self.send_wire(&WireMessage::Nack { seq, reason: NackReason::StaleOrReplayedSeq });
                    return;
                }
                let is_command = matches!(msg, BusMessage::Command { .. });
                match bus.send(msg) {
                    Ok(()) => {
                        self.last_uplink_seq = Some(seq);
                        if is_command {
                            let _ = self.pending_command_seqs.push_back(seq);
                        }
                        self.send_wire(&WireMessage::Ack { seq });
                    }
                    Err(_) => self.send_wire(&WireMessage::Nack { seq, reason: NackReason::BusFull }),
                }
            }
            Ok(other) => esp_println::println!("uart-link: unexpected message from ground: {other:?}"),
            Err(_) => esp_println::println!("uart-link: bad frame from ground"),
        }
    }
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    let mut led = Output::new(peripherals.GPIO8, Level::Low, OutputConfig::default());
    let mut delay = Delay::new();

    // Physical UART1, separate from the USB-JTAG serial console `esp_println`
    // uses for debug text — this one carries the real framed ground-link
    // protocol. GPIO4/GPIO5 are free (only GPIO8 is claimed, by the LED).
    let uart = Uart::new(peripherals.UART1, UartConfig::default())
        .expect("failed to configure UART1")
        .with_tx(peripherals.GPIO4)
        .with_rx(peripherals.GPIO5);

    let link = RefCell::new(UartLink::new(uart));
    let link_ref = &link;

    let mut scheduler = Scheduler::new(
        move |msg: &BusMessage| {
            match msg {
                BusMessage::Heartbeat { .. } => {
                    led.toggle();
                    link_ref.borrow_mut().poll_rx();
                }
                BusMessage::Telemetry { sensor_id, value } => {
                    esp_println::println!("telemetry: sensor={sensor_id} value={value:.2}");
                }
                BusMessage::Log { severity, source, text } => {
                    esp_println::println!("log: [{severity:?}] {source}: {text}");
                }
                BusMessage::Command { name, .. } => {
                    esp_println::println!("command: {name}");
                }
                BusMessage::Housekeeping { apps } => {
                    for app in apps {
                        esp_println::println!(
                            "hk: {} accepted={} rejected={} unhealthy_ticks={}",
                            app.name,
                            app.cmd_accepted,
                            app.cmd_rejected,
                            app.consecutive_tick_failures
                        );
                    }
                }
            }
            link_ref.borrow_mut().send_bus_message(msg);
        },
        move |outcome: &CommandOutcome| {
            esp_println::println!("command outcome: {outcome:?}");
            link_ref.borrow_mut().send_command_outcome(outcome);
        },
    );

    link.borrow_mut().bus = Some(scheduler.bus_handle());

    scheduler.register(AnyApp::Heartbeat(HeartbeatApp));
    scheduler.register(AnyApp::TempSensor(TempSensorApp::new(1)));
    scheduler.register(AnyApp::Battery(BatteryApp::new(2)));

    scheduler.run(u32::MAX, 500, &mut delay);

    unreachable!("u32::MAX ticks at 500ms won't elapse before the mission does");
}
