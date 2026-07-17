use std::{
    fs::{File, OpenOptions},
    io::{self, BufReader, Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

const TAIL_BLOCK_SIZE: usize = 8 * 1024;

/// Errors returned while writing or reading a JSON Lines log.
#[derive(Debug, Error)]
pub enum JsonlLogError {
    #[error("failed to open JSONL file `{path}`")]
    Open {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("I/O operation on JSONL file failed")]
    Io(#[from] io::Error),
    #[error("failed to write JSONL value")]
    Write(#[from] jsonl::WriteError),
    #[error("failed to read JSONL value")]
    Read(#[from] jsonl::ReadError),
}

/// A JSON Lines log that keeps its append handle open for its whole lifetime.
#[derive(Debug)]
pub struct JsonlLog {
    file: File,
}

/// A stateful reader that yields JSONL objects from newest to oldest.
///
/// A reader captures the file length when it is opened. Appends made after that point are not
/// visible through the same reader instance.
#[derive(Debug)]
pub struct LastReader<'a> {
    log: &'a mut JsonlLog,
    block: Vec<u8>,
    scan_end: usize,
    unread_end: u64,
    line_fragments: Vec<Vec<u8>>,
    bof_record_pending: bool,
    finished: bool,
}

impl JsonlLog {
    /// Opens `path` for appending, creating it when it does not already exist.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, JsonlLogError> {
        let path = path.as_ref();
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(path)
            .map_err(|source| JsonlLogError::Open {
                path: path.to_path_buf(),
                source,
            })?;

        Ok(Self { file })
    }

    /// Appends and synchronizes one JSON value and its terminating newline.
    ///
    /// No Rust-side output buffer is used. After `jsonl::write` completes, `sync_data` asks the
    /// operating system to persist the file's contents before this method returns.
    pub fn append<T: Serialize>(&mut self, value: &T) -> Result<(), JsonlLogError> {
        jsonl::write(&mut self.file, value)?;
        self.file.sync_data()?;
        Ok(())
    }

    /// Creates a stateful reader that returns objects from the end of this log.
    ///
    /// The reader mutably borrows this log, so appending and reverse-reading through the same file
    /// handle are deliberately sequential.
    pub fn last_reader(&mut self) -> Result<LastReader<'_>, JsonlLogError> {
        LastReader::new(self)
    }
}

impl<'a> LastReader<'a> {
    fn new(log: &'a mut JsonlLog) -> Result<Self, JsonlLogError> {
        let file_len = log.file.metadata()?.len();

        if file_len == 0 {
            return Ok(Self {
                log,
                block: Vec::new(),
                scan_end: 0,
                unread_end: 0,
                line_fragments: Vec::new(),
                bof_record_pending: false,
                finished: true,
            });
        }

        log.file.seek(SeekFrom::End(-1))?;
        let mut final_byte = [0_u8; 1];
        log.file.read_exact(&mut final_byte)?;

        Ok(Self {
            log,
            block: Vec::new(),
            scan_end: 0,
            // A terminal newline terminates the final record; it does not create a phantom record.
            unread_end: if final_byte[0] == b'\n' {
                file_len - 1
            } else {
                file_len
            },
            line_fragments: Vec::new(),
            bof_record_pending: true,
            finished: false,
        })
    }

    /// Reads one JSON object from the end of the snapshot.
    ///
    /// Successful calls return objects in reverse chronological order. `Ok(None)` indicates a
    /// stable end-of-file state; malformed JSON is returned as an error rather than treated as EOF.
    pub fn read_last_one<T: DeserializeOwned>(&mut self) -> Result<Option<T>, JsonlLogError> {
        let Some(mut line) = self.read_last_line()? else {
            return Ok(None);
        };

        // Use the JSONL crate for deserialization and make an empty physical line an invalid JSON
        // record rather than allowing it to be mistaken for the reader's EOF marker.
        line.push(b'\n');
        Ok(Some(jsonl::read(BufReader::new(Cursor::new(line)))?))
    }

    fn read_last_line(&mut self) -> Result<Option<Vec<u8>>, io::Error> {
        if self.finished {
            return Ok(None);
        }

        loop {
            if self.scan_end > 0 {
                let end = self.scan_end;
                if let Some(newline_index) =
                    (0..end).rev().find(|&index| self.block[index] == b'\n')
                {
                    self.line_fragments
                        .push(self.block[newline_index + 1..end].to_vec());
                    self.scan_end = newline_index;
                    return Ok(Some(self.take_line()));
                }

                self.line_fragments.push(self.block[..end].to_vec());
                self.scan_end = 0;
            }

            if self.unread_end == 0 {
                self.finished = true;
                if self.bof_record_pending {
                    self.bof_record_pending = false;
                    return Ok(Some(self.take_line()));
                }
                return Ok(None);
            }

            let block_len = self.unread_end.min(TAIL_BLOCK_SIZE as u64) as usize;
            let block_start = self.unread_end - block_len as u64;
            self.block.resize(block_len, 0);
            self.log.file.seek(SeekFrom::Start(block_start))?;
            self.log.file.read_exact(&mut self.block)?;
            self.scan_end = block_len;
            self.unread_end = block_start;
        }
    }

    fn take_line(&mut self) -> Vec<u8> {
        let fragments = std::mem::take(&mut self.line_fragments);
        let total_len = fragments.iter().map(Vec::len).sum();
        let mut line = Vec::with_capacity(total_len);
        for fragment in fragments.into_iter().rev() {
            line.extend_from_slice(&fragment);
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use serde::{Deserialize, Serialize};

    use super::JsonlLog;

    static NEXT_FILE_ID: AtomicUsize = AtomicUsize::new(0);

    #[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
    struct Event {
        id: u32,
        message: String,
    }

    fn test_path(name: &str) -> PathBuf {
        let id = NEXT_FILE_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("r1-jsonl-{name}-{}-{id}.jsonl", std::process::id()))
    }

    #[test]
    fn appends_multiple_values_with_one_open_handle() {
        let path = test_path("append");
        let mut log = JsonlLog::open(&path).unwrap();

        log.append(&Event {
            id: 1,
            message: "first".into(),
        })
        .unwrap();
        log.append(&Event {
            id: 2,
            message: "second".into(),
        })
        .unwrap();

        let mut reader = log.last_reader().unwrap();
        assert_eq!(reader.read_last_one::<Event>().unwrap().unwrap().id, 2);
        assert_eq!(reader.read_last_one::<Event>().unwrap().unwrap().id, 1);
        assert!(reader.read_last_one::<Event>().unwrap().is_none());
        drop(reader);

        drop(log);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn reopens_and_reads_persisted_values_from_newest_to_oldest() {
        let path = test_path("restart");
        {
            let mut log = JsonlLog::open(&path).unwrap();
            for id in 0..5 {
                log.append(&Event {
                    id,
                    message: format!("event-{id}"),
                })
                .unwrap();
            }
        }

        let mut log = JsonlLog::open(&path).unwrap();
        let mut reader = log.last_reader().unwrap();
        for expected_id in (0..5).rev() {
            assert_eq!(
                reader.read_last_one::<Event>().unwrap().unwrap().id,
                expected_id
            );
        }
        assert!(reader.read_last_one::<Event>().unwrap().is_none());

        drop(reader);
        drop(log);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn last_reader_reads_one_value_at_a_time_from_newest_to_oldest() {
        let path = test_path("last-reader");
        let mut log = JsonlLog::open(&path).unwrap();
        for id in 0..3 {
            log.append(&Event {
                id,
                message: format!("event-{id}"),
            })
            .unwrap();
        }

        let mut reader = log.last_reader().unwrap();
        assert_eq!(reader.read_last_one::<Event>().unwrap().unwrap().id, 2);
        assert_eq!(reader.read_last_one::<Event>().unwrap().unwrap().id, 1);
        assert_eq!(reader.read_last_one::<Event>().unwrap().unwrap().id, 0);
        assert!(reader.read_last_one::<Event>().unwrap().is_none());
        assert!(reader.read_last_one::<Event>().unwrap().is_none());

        drop(reader);
        drop(log);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn last_reader_releases_the_log_for_a_later_append() {
        let path = test_path("last-reader-sequential");
        let mut log = JsonlLog::open(&path).unwrap();
        log.append(&Event {
            id: 1,
            message: "first".into(),
        })
        .unwrap();
        log.append(&Event {
            id: 2,
            message: "second".into(),
        })
        .unwrap();

        {
            let mut reader = log.last_reader().unwrap();
            assert_eq!(reader.read_last_one::<Event>().unwrap().unwrap().id, 2);
            assert_eq!(reader.read_last_one::<Event>().unwrap().unwrap().id, 1);
            assert!(reader.read_last_one::<Event>().unwrap().is_none());
        }

        // The shared handle was sought backwards by the reader, but append mode still writes at EOF.
        log.append(&Event {
            id: 3,
            message: "later".into(),
        })
        .unwrap();

        {
            let mut reader = log.last_reader().unwrap();
            assert_eq!(reader.read_last_one::<Event>().unwrap().unwrap().id, 3);
            assert_eq!(reader.read_last_one::<Event>().unwrap().unwrap().id, 2);
            assert_eq!(reader.read_last_one::<Event>().unwrap().unwrap().id, 1);
            assert!(reader.read_last_one::<Event>().unwrap().is_none());
        }

        drop(log);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn reads_a_final_record_without_a_terminal_newline() {
        let path = test_path("no-final-newline");
        fs::write(
            &path,
            "{\"id\":1,\"message\":\"你好\"}\n{\"id\":2,\"message\":\"再见\"}",
        )
        .unwrap();

        let mut log = JsonlLog::open(&path).unwrap();
        let mut reader = log.last_reader().unwrap();
        assert_eq!(reader.read_last_one::<Event>().unwrap().unwrap().id, 2);
        assert_eq!(reader.read_last_one::<Event>().unwrap().unwrap().id, 1);
        assert!(reader.read_last_one::<Event>().unwrap().is_none());

        drop(reader);
        drop(log);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn reads_a_record_larger_than_the_reverse_scan_block() {
        let path = test_path("large-record");
        let large_message = "x".repeat(super::TAIL_BLOCK_SIZE * 2);
        let mut log = JsonlLog::open(&path).unwrap();
        log.append(&Event {
            id: 1,
            message: "old".into(),
        })
        .unwrap();
        log.append(&Event {
            id: 2,
            message: large_message.clone(),
        })
        .unwrap();

        let mut reader = log.last_reader().unwrap();
        assert_eq!(
            reader.read_last_one::<Event>().unwrap(),
            Some(Event {
                id: 2,
                message: large_message,
            })
        );
        assert_eq!(reader.read_last_one::<Event>().unwrap().unwrap().id, 1);
        assert!(reader.read_last_one::<Event>().unwrap().is_none());

        drop(reader);
        drop(log);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn returns_an_error_for_a_blank_jsonl_record() {
        let path = test_path("blank-record");
        fs::write(&path, "{\"id\":1,\"message\":\"ok\"}\n\n").unwrap();

        let mut log = JsonlLog::open(&path).unwrap();
        let mut reader = log.last_reader().unwrap();
        assert!(reader.read_last_one::<Event>().is_err());
        assert_eq!(reader.read_last_one::<Event>().unwrap().unwrap().id, 1);
        assert!(reader.read_last_one::<Event>().unwrap().is_none());

        drop(reader);
        drop(log);
        fs::remove_file(path).unwrap();
    }
}
