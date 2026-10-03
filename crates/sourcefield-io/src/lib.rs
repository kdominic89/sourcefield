//! Bounded native byte acquisition and SHA-256 mechanics shared by actual I/O callers.
//!
//! These functions make no path, symlink, provenance, or publication-policy claims.
//! Callers open and admit their own resources and retain their boundary diagnostics.
#![deny(missing_docs)]
#![forbid(unsafe_code)]

use std::io::{self, Read, Write};

use anyhow::{Result, ensure};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

const BUFFER_BYTES: usize = 64 * 1024;

/// Read at most the stated byte count, using one extra byte only to detect overflow.
/// Size hints from untrusted metadata never determine a large up-front allocation.
pub fn read_bounded(reader: impl Read, max_bytes: u64) -> io::Result<Vec<u8>> {
    read_into_bounded(reader, max_bytes, Vec::new())
}

/// Read an opened file with a size-admitted allocation while retaining the growth sentinel.
/// Metadata larger than the caller's limit is rejected before allocation. This method does
/// not admit symlinks or regular-file policy; the caller owns those filesystem boundaries.
pub fn read_file_bounded(file: std::fs::File, max_bytes: u64) -> io::Result<Vec<u8>> {
    let length = file.metadata()?.len();

    if length > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input exceeds configured byte limit",
        ));
    }

    let capacity = usize::try_from(length).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "file length exceeds addressable memory",
        )
    })?;

    read_into_bounded(file, max_bytes, Vec::with_capacity(capacity))
}

/// Share overflow detection while preserving the allocation strategy of each real caller.
fn read_into_bounded(reader: impl Read, max_bytes: u64, mut bytes: Vec<u8>) -> io::Result<Vec<u8>> {
    let sentinel_limit = max_bytes.checked_add(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "byte limit cannot reserve an overflow sentinel",
        )
    })?;

    reader.take(sentinel_limit).read_to_end(&mut bytes)?;

    if bytes.len() as u64 > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "input exceeds configured byte limit",
        ));
    }

    Ok(bytes)
}

/// Read a bounded UTF-8 document, reusing its acquired byte allocation for the string.
pub fn read_utf8_bounded(reader: impl Read, max_bytes: u64) -> io::Result<String> {
    let bytes = read_bounded(reader, max_bytes)?;

    String::from_utf8(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.utf8_error()))
}

/// Parse bounded JSON directly from a buffered stream without retaining its complete bytes.
/// JSON syntax, trailing data, and size violations remain errors instead of partial values.
pub fn read_json_bounded<T: DeserializeOwned>(reader: impl Read, max_bytes: u64) -> Result<T> {
    let sentinel_limit = max_bytes.checked_add(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "byte limit cannot reserve an overflow sentinel",
        )
    })?;
    let mut reader = io::BufReader::new(reader.take(sentinel_limit));
    let value = serde_json::from_reader(&mut reader)?;

    ensure!(
        reader.get_ref().limit() > 0,
        "JSON input exceeds configured byte limit"
    );

    Ok(value)
}

/// Compute the unchanged lowercase hexadecimal SHA-256 identity of in-memory bytes.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Hash a stream with a fixed-size buffer, without buffering the entire input.
pub fn sha256_reader(reader: impl Read) -> io::Result<String> {
    copy_sha256(reader, io::sink())
}

/// Copy and hash a stream using fixed-size memory; callers own flush and durability policy.
pub fn copy_sha256(mut reader: impl Read, mut writer: impl Write) -> io::Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; BUFFER_BYTES];

    loop {
        let count = match reader.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };

        if count == 0 {
            break;
        }

        writer.write_all(&buffer[..count])?;
        hash.update(&buffer[..count]);
    }

    Ok(hex::encode(hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_byte_limit_is_accepted() {
        let input = b"abcd";

        let bytes = read_bounded(input.as_slice(), 4).unwrap();

        assert_eq!(bytes, input);
    }

    #[test]
    fn limit_plus_one_is_rejected() {
        let input = b"abcde";

        let result = read_bounded(input.as_slice(), 4);

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn zero_limit_accepts_only_empty_input() {
        let input = io::empty();

        let result = read_bounded(input, 0).unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn utf8_is_validated_without_lossy_replacement() {
        let input = [0xff];

        let result = read_utf8_bounded(input.as_slice(), 1);

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn streaming_json_accepts_exact_limit() {
        let input = b"[1,2]";

        let value: Vec<u8> = read_json_bounded(input.as_slice(), 5).unwrap();

        assert_eq!(value, vec![1, 2]);
    }

    #[test]
    fn streaming_json_rejects_overflow_whitespace() {
        let input = b"[1,2] ";

        let result = read_json_bounded::<Vec<u8>>(input.as_slice(), 5);

        assert!(result.is_err());
    }

    #[test]
    fn malformed_json_is_rejected() {
        let input = b"[broken";

        let result = read_json_bounded::<Vec<u8>>(input.as_slice(), 20);

        assert!(result.is_err());
    }

    #[test]
    fn trailing_json_is_rejected() {
        let input = b"[] []";

        let result = read_json_bounded::<Vec<u8>>(input.as_slice(), 20);

        assert!(result.is_err());
    }

    #[test]
    fn stream_hash_matches_known_protocol() {
        let input = b"abc";

        let digest = sha256_reader(input.as_slice()).unwrap();

        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(digest, sha256_bytes(input));
    }

    #[test]
    fn copy_hash_preserves_stream_bytes() {
        let input = b"captured bytes";
        let mut output = Vec::new();

        let digest = copy_sha256(input.as_slice(), &mut output).unwrap();

        assert_eq!(output, input);
        assert_eq!(digest, sha256_bytes(input));
    }

    /// Independent handle cleanup keeps size-admission tests portable to Windows.
    struct TestFile(std::path::PathBuf);

    impl TestFile {
        fn create(bytes: &[u8]) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "sourcefield-io-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));

            std::fs::write(&path, bytes).unwrap();

            Self(path)
        }
    }

    impl Drop for TestFile {
        fn drop(&mut self) {
            std::fs::remove_file(&self.0).unwrap();
        }
    }

    #[test]
    fn admitted_file_uses_exact_payload_capacity() {
        let fixture = TestFile::create(&[42; 65536]);
        let file = std::fs::File::open(&fixture.0).unwrap();

        let bytes = read_file_bounded(file, 65536).unwrap();

        assert_eq!(bytes.len(), 65536);
        assert_eq!(bytes.capacity(), 65536);
    }

    #[test]
    fn oversized_file_fails_before_reading() {
        let fixture = TestFile::create(b"five!");
        let file = std::fs::File::open(&fixture.0).unwrap();

        let result = read_file_bounded(file, 4);

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn input_growth_beyond_admitted_capacity_still_obeys_limit() {
        let original_size = 4;
        let grown_input = b"abcde";
        let bytes = Vec::with_capacity(original_size);

        let result = read_into_bounded(grown_input.as_slice(), original_size as u64, bytes);

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn overflowing_limit_is_rejected_without_reading() {
        let input = io::repeat(0);

        let result = read_bounded(input, u64::MAX);

        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }
}
