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
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    let mut led = Output::new(peripherals.GPIO8, Level::Low, OutputConfig::default());
    let mut delay = Delay::new();

    let mut scheduler = Scheduler::new(
        move |msg: &BusMessage| {
            if let BusMessage::Heartbeat { .. } = msg {
                led.toggle();
            }
        },
        |_outcome| {},
    );

    scheduler.register(AnyApp::Heartbeat(HeartbeatApp));
    scheduler.register(AnyApp::TempSensor(TempSensorApp::new(1)));
    scheduler.register(AnyApp::Battery(BatteryApp::new(2)));

    scheduler.run(u32::MAX, 500, &mut delay);

    loop {}
}