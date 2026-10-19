#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| eks::fuzzing::webauthn_assertion(data));
