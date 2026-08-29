//! An ed25519 key as OpenSSH writes one.
//!
//! **A format question, not a cryptographic one.** An `ssh-ed25519` key *is*
//! an ed25519 key: the same curve, the same 32-byte seed, the same 64-byte
//! signatures. What the format buys is that the file is a real key file — one
//! `ssh-keygen -y` will read, a password manager will hold beside the others,
//! and a person can look at and recognise — rather than sixty-four hex
//! characters that only this game knows what to do with.
//!
//! That matters here for a reason beyond tidiness. A key kept as a string in a
//! text box is a key that gets typed over; a key kept as a file is a thing you
//! have. See [`super::person::Key`].
//!
//! ## What is and is not supported
//!
//! Unencrypted keys only. A passphrase-protected key is `bcrypt`-KDF and AES
//! over the same bytes, and half-implementing a KDF to open somebody's real
//! key is a worse idea than asking them to decrypt it first — which is one
//! `ssh-keygen -p` and a message that says so.
//!
//! Everything written here is **deterministic**: the same key produces the
//! same file, byte for byte. OpenSSH puts a random pair of check integers in
//! the private blob to detect a wrong passphrase, and with no passphrase there
//! is nothing to detect, so those are derived from the public key instead. A
//! file that changes every time it is written is a file that looks modified
//! when it is not.

/// The one key type here. Ed25519 by name, in the format's own vocabulary.
const ALGORITHM: &str = "ssh-ed25519";
const MAGIC: &[u8] = b"openssh-key-v1\0";
const BEGIN: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";
const END: &str = "-----END OPENSSH PRIVATE KEY-----";
/// What OpenSSH wraps its base64 at.
const WRAP: usize = 70;

/// A length-prefixed field, which is what "string" means in this format.
fn field(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

/// Read one back, and how much was consumed.
fn take<'a>(bytes: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let len = u32::from_be_bytes(bytes.get(*at..*at + 4)?.try_into().ok()?) as usize;
    let from = *at + 4;
    let out = bytes.get(from..from.checked_add(len)?)?;
    *at = from + len;
    Some(out)
}

/// The public half, as it would appear in `authorized_keys`.
pub fn public(verifying: &[u8; 32], comment: &str) -> String {
    let mut blob = Vec::new();
    field(&mut blob, ALGORITHM.as_bytes());
    field(&mut blob, verifying);
    format!("{ALGORITHM} {} {comment}", base64::encode(&blob))
}

/// The private half, as an `openssh-key-v1` file.
pub fn private(seed: &[u8; 32], verifying: &[u8; 32], comment: &str) -> String {
    let mut pub_blob = Vec::new();
    field(&mut pub_blob, ALGORITHM.as_bytes());
    field(&mut pub_blob, verifying);

    // Derived rather than random: see the module note on determinism.
    let check = &verifying[..4];
    let mut secret = Vec::new();
    secret.extend_from_slice(check);
    secret.extend_from_slice(check);
    field(&mut secret, ALGORITHM.as_bytes());
    field(&mut secret, verifying);
    let mut private = seed.to_vec();
    private.extend_from_slice(verifying);
    field(&mut secret, &private);
    field(&mut secret, comment.as_bytes());
    // Padded to the cipher's block size with 1, 2, 3… even when the cipher is
    // "none", which the format requires and `ssh-keygen` checks.
    for i in 1..=((8 - secret.len() % 8) % 8) {
        secret.push(i as u8);
    }

    let mut out = MAGIC.to_vec();
    field(&mut out, b"none");
    field(&mut out, b"none");
    field(&mut out, b"");
    out.extend_from_slice(&1u32.to_be_bytes());
    field(&mut out, &pub_blob);
    field(&mut out, &secret);

    let body = base64::encode(&out);
    let mut text = String::from(BEGIN);
    for line in body.as_bytes().chunks(WRAP) {
        text.push('\n');
        text.push_str(std::str::from_utf8(line).expect("base64 is ascii"));
    }
    text.push('\n');
    text.push_str(END);
    text.push('\n');
    text
}

/// The seed out of an `openssh-key-v1` file, or why it could not be had.
pub fn seed_of(text: &str) -> Result<[u8; 32], String> {
    let body: String = text
        .lines()
        .map(str::trim)
        .skip_while(|l| *l != BEGIN)
        .skip(1)
        .take_while(|l| *l != END)
        .collect();
    if body.is_empty() {
        return Err("that is not an OpenSSH private key file".into());
    }
    let bytes = base64::decode(&body).ok_or("the key file is not valid base64")?;
    let rest = bytes.strip_prefix(MAGIC).ok_or("the key file is not openssh-key-v1")?;

    let mut at = 0;
    let cipher = take(rest, &mut at).ok_or("the key file is truncated")?;
    if cipher != b"none" {
        // Named, because "it did not work" is not the same message as "run one
        // command and it will".
        return Err(
            "that key has a passphrase. Decrypt a copy first: ssh-keygen -p -N '' -f <file>".into(),
        );
    }
    take(rest, &mut at).ok_or("the key file is truncated")?; // kdf name
    take(rest, &mut at).ok_or("the key file is truncated")?; // kdf options
    at += 4; // one key, always
    take(rest, &mut at).ok_or("the key file is truncated")?; // public blob

    let secret = take(rest, &mut at).ok_or("the key file is truncated")?;
    let mut at = 8; // the two check integers
    let algorithm = take(secret, &mut at).ok_or("the key file is truncated")?;
    if algorithm != ALGORITHM.as_bytes() {
        return Err(format!(
            "that is a {} key; this game uses {ALGORITHM}",
            String::from_utf8_lossy(algorithm)
        ));
    }
    take(secret, &mut at).ok_or("the key file is truncated")?; // public half again
    let private = take(secret, &mut at).ok_or("the key file is truncated")?;
    // seed || public, and only the first half is the secret.
    private.get(..32).and_then(|s| s.try_into().ok()).ok_or_else(|| "the key is malformed".into())
}

/// Just enough base64 for one key file, because one key file is all this needs
/// and a dependency for it would be a dependency to audit.
mod base64 {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
            for i in 0..4 {
                out.push(if i <= chunk.len() {
                    ALPHABET[(n >> (18 - i * 6)) as usize & 63] as char
                } else {
                    '='
                });
            }
        }
        out
    }

    pub fn decode(text: &str) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(text.len() / 4 * 3);
        let mut acc = 0u32;
        let mut bits = 0;
        for c in text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
            let value = ALPHABET.iter().position(|a| *a == c)? as u32;
            acc = (acc << 6) | value;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> [u8; 32] {
        std::array::from_fn(|i| (i * 7 + 3) as u8)
    }

    fn verifying() -> [u8; 32] {
        std::array::from_fn(|i| (i * 11 + 5) as u8)
    }

    /// The whole of what the format has to do here: what is written comes
    /// back, so a key file carried to another machine is the same person.
    #[test]
    fn a_key_file_reads_back_as_the_key_that_wrote_it() {
        let text = private(&seed(), &verifying(), "conwayskingdom");
        assert_eq!(seed_of(&text).unwrap(), seed());
    }

    /// The same key writes the same file. A file that changed every time it
    /// was saved would look modified when nothing had happened to it, and this
    /// is a file people are meant to keep and compare.
    #[test]
    fn writing_the_same_key_twice_gives_the_same_bytes() {
        let once = private(&seed(), &verifying(), "conwayskingdom");
        assert_eq!(once, private(&seed(), &verifying(), "conwayskingdom"));
    }

    /// It looks like what it claims to be. Not decoration: the armour and the
    /// line width are what makes `ssh-keygen` and every editor treat it as a
    /// key rather than as a blob.
    #[test]
    fn a_key_file_is_shaped_like_one() {
        let text = private(&seed(), &verifying(), "conwayskingdom");
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines[0], BEGIN);
        assert_eq!(*lines.last().unwrap(), END);
        assert!(lines[1..lines.len() - 1].iter().all(|l| l.len() <= WRAP), "{text}");
        assert!(text.ends_with('\n'), "a file without a trailing newline");
    }

    /// The public half is what goes in `authorized_keys`, and its base64
    /// always starts the same way because the algorithm name is the first
    /// field of every one of them.
    #[test]
    fn the_public_half_is_an_authorized_keys_line() {
        let line = public(&verifying(), "hugh@laptop");
        assert!(line.starts_with("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI"), "{line}");
        assert!(line.ends_with(" hugh@laptop"));
        assert_eq!(line.split(' ').count(), 3);
    }

    /// A passphrase is answered with the command that removes it, because "it
    /// did not work" and "run this one thing" are different messages and only
    /// one of them is any use standing in front of the failure.
    #[test]
    fn a_passphrase_is_named_rather_than_refused_blankly() {
        let mut text = private(&seed(), &verifying(), "c");
        // The cipher name field, swapped for one that is not "none".
        let body: String = text.lines().skip(1).take_while(|l| *l != END).collect();
        let mut bytes = base64::decode(&body).unwrap();
        let at = MAGIC.len() + 4;
        bytes[at..at + 4].copy_from_slice(b"aes2");
        text = format!("{BEGIN}\n{}\n{END}\n", base64::encode(&bytes));
        let why = seed_of(&text).unwrap_err();
        assert!(why.contains("ssh-keygen -p"), "{why}");
    }

    /// Rubbish is a message rather than a panic. It arrives by paste and by
    /// file picker, and anybody may hand over anything.
    #[test]
    fn nonsense_is_refused_rather_than_fatal() {
        for bad in [
            "",
            "hello",
            BEGIN,
            &format!("{BEGIN}\nnot base64 !!!\n{END}"),
            &format!("{BEGIN}\n{}\n{END}", base64::encode(b"wrong magic entirely")),
        ] {
            assert!(seed_of(bad).is_err(), "{bad:?} was accepted");
        }
    }

    /// The base64 here is written by hand, so it is checked against known
    /// answers rather than against itself.
    #[test]
    fn base64_matches_the_standard() {
        for (raw, encoded) in [
            (&b""[..], ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
        ] {
            assert_eq!(base64::encode(raw), encoded);
            assert_eq!(base64::decode(encoded).unwrap(), raw);
        }
    }
}
