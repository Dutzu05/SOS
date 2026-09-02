#![no_std]
#![no_main]

use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::main;

use sos_core::apps::{AnyApp, BatteryApp, HeartbeatApp, TempSensorApp};
use sos_core::{BusMessage, Scheduler};

esp_bootloader_esp_idf::esp_app_desc!();

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    esp_println::println!("panic: {info}");
    loop {}
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    let mut led = Output::new(peripherals.GPIO8, Level::Low, OutputConfig::default());
    let mut delay = Delay::new();

    let mut scheduler = Scheduler::new(
        move |msg: &BusMessage| {
            match msg {
                BusMessage::Heartbeat { .. } => led.toggle(),
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
        },
        |outcome| esp_println::println!("command outcome: {outcome:?}"),
    );

    scheduler.register(AnyApp::Heartbeat(HeartbeatApp));
    scheduler.register(AnyApp::TempSensor(TempSensorApp::new(1)));
    scheduler.register(AnyApp::Battery(BatteryApp::new(2)));

    scheduler.run(u32::MAX, 500, &mut delay);

    loop {}
}