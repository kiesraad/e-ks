# Enrolling a security key for the CSB login

Committee members log in to the CSB section with a security key (a YubiKey
with a PIN). Keys are registered in advance: each key is enrolled once with
the script below, and the administrator puts the resulting line in the
`CSB_WEBAUTHN_USERS` environment variable. There is no registration page in
e-KS and nothing is stored at runtime.

## What you need

- The security key, with a PIN set. Without a PIN enrolment fails. Set one
  with `fido2-token -S <device>` or `ykman fido access change-pin`.
- The libfido2 command-line tools on the machine the key is plugged into:
  `apt install fido2-tools` (Debian/Ubuntu), `brew install libfido2` (macOS).
  On Linux the key must be accessible without root; libfido2 ships udev
  rules for that (`70-u2f.rules`).
- The host of the CSB origin, i.e. `CSB_WEBAUTHN_ORIGIN` without the scheme:
  `csb.example.nl` for `https://csb.example.nl`, `localhost` for a
  development server. A key enrolled for one host only works there.
- The username agreed with the administrator: 1 to 64 letters, digits, `.`,
  `-`, `_` or `@`. It appears in the audit log.

## Enrol

Plug in the key and run, from the repository root:

```sh
bin/enrol_csb_user alice csb.example.nl
```

With more than one key attached, add the device path from `fido2-token -L`
as third argument. Enter the PIN when asked and touch the key when it blinks.
The script prints one line:

```
alice:zGJB3Yi...:MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE...
```

That is `username:credential-id:public-key`. It contains no secrets (the
private key never leaves the security key), so it can be sent to the
administrator by any channel that preserves it exactly.

## Configure

The administrator adds the line to `CSB_WEBAUTHN_USERS`, comma-separated
when there are several, and restarts e-KS:

```sh
CSB_WEBAUTHN_ORIGIN=https://csb.example.nl
CSB_WEBAUTHN_USERS=alice:zGJB3Yi...:MFkw...,bob:5Qk1Lx...:MFkw...
```

Startup refuses the whole list on a malformed entry, a username listed
twice, or a key listed for two users, so a typo cannot silently drop a user.
To revoke a key, remove its line. To replace a lost key, enrol the new one and
replace the line. A member with two keys gets two lines with different
usernames.

## Without the script

The script only wraps `fido2-cred`. By hand:

```sh
{ head -c 32 /dev/urandom | base64; echo csb.example.nl; echo alice; head -c 32 /dev/urandom | base64; } > param
fido2-cred -M -v -i param /dev/hidraw5 | fido2-cred -V -v -o cred
```

`cred` then holds the credential id (first line, base64) followed by the
public key as PEM. The configuration line is the username, the credential
id, and the PEM body without its header and footer lines joined into one
line, separated by colons.
