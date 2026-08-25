#![no_main]
use libfuzzer_sys::fuzz_target;
use sos_core::BusMessage;

fuzz_target!(|data: &[u8]| {
    let mut buf = data.to_vec();
    // We don't care about the Ok case here — just that malformed input
    // never panics or crashes. A clean Err is success.
    let _ = BusMessage::from_frame(&mut buf);
});