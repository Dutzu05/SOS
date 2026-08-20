use std::sync::mpsc;
use std::thread;

// This struct represents a message on our software bus.
// Think of it as a tiny "telemetry packet."
#[derive(Debug)]
struct SensorReading {
    sensor_id: u8,
    value: f32,
}

fn main() {
    // Create a channel. `tx` (transmitter) is how apps send messages.
    // `rx` (receiver) is how an app receives them.
    let (tx, rx) = mpsc::channel::<SensorReading>();

    // Spawn a thread to simulate our "sensor app" running independently.
    let sensor_app = thread::spawn(move || {
        let reading = SensorReading { sensor_id: 1, value: 23.7 };
        println!("[sensor_app] sending: {:?}", reading);
        tx.send(reading).unwrap();
        // Try uncommenting the next line after your first successful run:
        println!("{:?}", reading);
    });

    // Meanwhile, our "main" app acts as the receiver.
    let received = rx.recv().unwrap();
    println!("[main_app] received: {:?}", received);

    sensor_app.join().unwrap();
}