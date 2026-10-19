#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| eks::fuzzing::store_event_decode(data));
