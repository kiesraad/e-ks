#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| auth_service::fuzzing::saml_xsw(data));
