#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| eks::fuzzing::proxy_protocol_v2(data));
