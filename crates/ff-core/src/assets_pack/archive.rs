//! Opening a pack's zip without scanning the whole file.
//!
//! In `zip` 9, `ZipArchive::new` finds where the archive starts by
//! searching the file for central directory signatures, and that reads
//! every byte of it. For `music.pak` that was the whole
//! 380 MB, read and unmasked on the game thread before the title screen:
//! about two seconds of every launch from the 2026-10-22 snapshot on, when
//! the crate went from 2.4 to 9.
//!
//! A pack's zip is written by [`super::write_pack`] and always starts at the
//! first byte of the payload, so there is nothing to detect: the archive
//! offset is 0, and opening reads only the directory at the end.

use std::io::{Read, Seek};

use zip::read::{ArchiveOffset, Config};
use zip::result::ZipResult;
use zip::ZipArchive;

/// The zip inside a pack, whose archive starts at payload offset 0.
pub(super) fn open_pack_archive<R: Read + Seek>(reader: R) -> ZipResult<ZipArchive<R>> {
    let config = Config {
        archive_offset: ArchiveOffset::Known(0),
    };
    ZipArchive::with_config(config, reader)
}

#[cfg(test)]
mod tests {
    use std::io::{self, Cursor, SeekFrom, Write};

    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    use super::*;

    /// Counts the bytes a reader hands out.
    struct Counting<R> {
        inner: R,
        bytes: u64,
    }

    impl<R: Read> Read for Counting<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.bytes += n as u64;
            Ok(n)
        }
    }

    impl<R: Seek> Seek for Counting<R> {
        fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
            self.inner.seek(to)
        }
    }

    #[test]
    fn opening_reads_the_directory_not_the_tracks() {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let track = vec![0x5a_u8; 256 * 1024];
        for i in 0..32 {
            writer
                .start_file(format!("music/track_{i:02}.ogg"), stored)
                .unwrap();
            writer.write_all(&track).unwrap();
        }
        let zip = writer.finish().unwrap().into_inner();
        let total = zip.len() as u64;

        let mut reader = Counting {
            inner: Cursor::new(zip),
            bytes: 0,
        };
        let mut archive = open_pack_archive(&mut reader).unwrap();
        assert_eq!(archive.len(), 32);
        let mut bytes = Vec::new();
        archive
            .by_name("music/track_17.ogg")
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, track);
        drop(archive);

        // The directory plus one track, not the 8 MB of the whole file.
        assert!(
            reader.bytes < total / 8,
            "opening read {} of {} bytes",
            reader.bytes,
            total
        );
    }
}
