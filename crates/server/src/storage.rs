//! [`FileStorage`]: a node's durable state in two files in one directory.
//!
//! - `vote` holds the term and the vote. It is replaced whole: written to
//!   `vote.tmp`, flushed, then renamed over the old one.
//! - `log` holds one line per entry and only ever grows at the end or is cut
//!   back and regrown.
//! - `LOCK` is held for as long as the process runs, so two processes cannot
//!   share a directory.
//!
//! Every line is `<checksum> <json>`, the checksum being the 64-bit FNV-1a
//! of the JSON as 16 hexadecimal digits. A last line that was cut short by a
//! crash is removed when the log is opened: it was never flushed, so nothing
//! was ever told about it. A damaged line anywhere else is an error, and the
//! node does not start.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::hash::Hasher as _;
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use det::{Durable, Entry, Fnv1a, Index, NodeId, Term};
use host::storage::Storage;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// Why the files could not be read or written.
#[derive(Debug)]
pub enum StorageError {
    /// The operating system refused.
    Io {
        /// What was being done.
        doing: String,
        /// What went wrong.
        source: io::Error,
    },
    /// A file was read but its contents are damaged.
    Damaged(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { doing, source } => write!(f, "{doing}: {source}"),
            Self::Damaged(what) => write!(f, "{what}"),
        }
    }
}

/// Wraps an I/O error with what was being done.
fn io_error(doing: impl Into<String>) -> impl FnOnce(io::Error) -> StorageError {
    let doing = doing.into();
    move |source| StorageError::Io { doing, source }
}

/// The contents of the `vote` file.
#[derive(Serialize, Deserialize)]
struct Vote {
    /// The node's term.
    term: Term,
    /// Its vote in that term.
    voted_for: Option<NodeId>,
}

/// One line: the checksum of `json`, a space, `json`, a newline.
fn line(json: &str) -> String {
    let mut hasher = Fnv1a::new();
    hasher.write(json.as_bytes());
    format!("{:016x} {json}\n", hasher.finish())
}

/// The JSON of a line (given without its newline), if its checksum matches.
fn checked(text: &[u8]) -> Option<&str> {
    let text = std::str::from_utf8(text).ok()?;
    let (sum, json) = text.split_once(' ')?;
    (line(json) == format!("{sum} {json}\n")).then_some(json)
}

/// Flushes a directory, so a file created or renamed in it survives a crash.
fn sync_dir(dir: &Path) -> io::Result<()> {
    // Windows cannot open a directory to flush it, and does not need to.
    if cfg!(unix) {
        File::open(dir)?.sync_all()?;
    }
    Ok(())
}

/// A node's durable state on disk. See the module documentation.
#[derive(Debug)]
pub struct FileStorage<C> {
    /// The directory holding the files.
    dir: PathBuf,
    /// The open log file.
    log: File,
    /// The byte offset just past each stored entry, in order.
    ends: Vec<u64>,
    /// Held to keep other processes out of `dir`.
    _lock: File,
    /// The command type stored in the log.
    _commands: PhantomData<C>,
}

impl<C> FileStorage<C> {
    /// Opens the store in `dir`, creating the directory and files if this is
    /// the first time.
    ///
    /// # Errors
    /// If the directory cannot be created or opened, or another process
    /// holds it.
    pub fn open(dir: &Path) -> Result<Self, StorageError> {
        let shown = dir.display();
        fs::create_dir_all(dir).map_err(io_error(format!("creating {shown}")))?;
        let lock =
            File::create(dir.join("LOCK")).map_err(io_error(format!("opening {shown}/LOCK")))?;
        lock.try_lock().map_err(|e| {
            StorageError::Damaged(format!("{shown} is in use by another process ({e})"))
        })?;
        let log = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("log"))
            .map_err(io_error(format!("opening {shown}/log")))?;
        sync_dir(dir).map_err(io_error(format!("flushing {shown}")))?;
        Ok(Self {
            dir: dir.to_owned(),
            log,
            ends: Vec::new(),
            _lock: lock,
            _commands: PhantomData,
        })
    }
}

impl<C: Serialize + DeserializeOwned> FileStorage<C> {
    /// Reads the `vote` file; a missing one is a brand new node.
    fn load_vote(&self) -> Result<Vote, StorageError> {
        let path = self.dir.join("vote");
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(Vote {
                    term: Term(0),
                    voted_for: None,
                });
            }
            Err(e) => return Err(io_error(format!("reading {}", path.display()))(e)),
        };
        bytes
            .strip_suffix(b"\n")
            .and_then(checked)
            .and_then(|json| serde_json::from_str(json).ok())
            .ok_or_else(|| StorageError::Damaged(format!("{} is damaged", path.display())))
    }

    /// Reads the whole log, noting where each entry ends, and removes a last
    /// line that a crash cut short.
    fn load_log(&mut self) -> Result<Vec<Entry<C>>, StorageError> {
        let path = self.dir.join("log");
        let shown = path.display();
        let mut bytes = Vec::new();
        self.log
            .seek(SeekFrom::Start(0))
            .map_err(io_error(format!("reading {shown}")))?;
        self.log
            .read_to_end(&mut bytes)
            .map_err(io_error(format!("reading {shown}")))?;

        let mut entries = Vec::new();
        self.ends.clear();
        let mut start = 0;
        while let Some(length) = bytes[start..].iter().position(|b| *b == b'\n') {
            let entry = checked(&bytes[start..start + length])
                .and_then(|json| serde_json::from_str(json).ok())
                .ok_or_else(|| {
                    StorageError::Damaged(format!(
                        "{shown}: entry {} is damaged",
                        entries.len() + 1
                    ))
                })?;
            entries.push(entry);
            start += length + 1;
            self.ends.push(start as u64);
        }
        if start < bytes.len() {
            // No newline: the write was cut short, so it was never flushed.
            self.log
                .set_len(start as u64)
                .map_err(io_error(format!("repairing {shown}")))?;
            self.log
                .sync_all()
                .map_err(io_error(format!("repairing {shown}")))?;
        }
        Ok(entries)
    }
}

impl<C: Serialize + DeserializeOwned> Storage<C> for FileStorage<C> {
    type Error = StorageError;

    fn load(&mut self) -> Result<Durable<C>, StorageError> {
        let vote = self.load_vote()?;
        let log = self.load_log()?;
        Ok(Durable::new(vote.term, vote.voted_for, log))
    }

    fn save_vote(&mut self, term: Term, voted_for: Option<NodeId>) -> Result<(), StorageError> {
        let (path, temporary) = (self.dir.join("vote"), self.dir.join("vote.tmp"));
        let doing = || format!("writing {}", path.display());
        let json = serde_json::to_string(&Vote { term, voted_for }).expect("a vote serialises");
        let mut file = File::create(&temporary).map_err(io_error(doing()))?;
        file.write_all(line(&json).as_bytes())
            .map_err(io_error(doing()))?;
        file.sync_all().map_err(io_error(doing()))?;
        fs::rename(&temporary, &path).map_err(io_error(doing()))?;
        sync_dir(&self.dir).map_err(io_error(doing()))
    }

    fn write_log(&mut self, keep: Index, entries: &[Entry<C>]) -> Result<(), StorageError> {
        let doing = || format!("writing {}", self.dir.join("log").display());
        let keep = usize::try_from(keep.0).expect("log position fits in usize");
        assert!(
            keep <= self.ends.len(),
            "storage: keeping {keep} of {} entries",
            self.ends.len()
        );
        let mut end = keep.checked_sub(1).map_or(0, |last| self.ends[last]);
        if keep < self.ends.len() {
            self.ends.truncate(keep);
            self.log.set_len(end).map_err(io_error(doing()))?;
        }
        let mut bytes = Vec::new();
        for entry in entries {
            let json = serde_json::to_string(entry)
                .map_err(|e| StorageError::Damaged(format!("an entry does not serialise: {e}")))?;
            bytes.extend_from_slice(line(&json).as_bytes());
            self.ends.push(end + bytes.len() as u64);
        }
        end += bytes.len() as u64;
        debug_assert_eq!(self.ends.last().copied().unwrap_or(0), end);
        self.log.seek(SeekFrom::End(0)).map_err(io_error(doing()))?;
        self.log.write_all(&bytes).map_err(io_error(doing()))?;
        self.log.sync_data().map_err(io_error(doing()))
    }
}

#[cfg(test)]
mod tests {
    use det::Payload;

    use super::*;

    /// A fresh directory under the system's temporary one, removed on drop.
    pub(crate) struct TempDir(pub PathBuf);

    impl TempDir {
        pub(crate) fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("airport-server-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn entry(term: u64, command: u32) -> Entry<u32> {
        Entry {
            term: Term(term),
            payload: Payload::Command(command),
        }
    }

    fn reopen(dir: &TempDir) -> (FileStorage<u32>, Durable<u32>) {
        let mut storage = FileStorage::open(&dir.0).unwrap();
        let durable = storage.load().unwrap();
        (storage, durable)
    }

    #[test]
    fn a_new_directory_is_a_brand_new_node() {
        let dir = TempDir::new("new");
        let (_, durable) = reopen(&dir);
        assert_eq!(durable, Durable::new(Term(0), None, vec![]));
    }

    #[test]
    fn what_is_written_is_read_back_after_a_restart() {
        let dir = TempDir::new("roundtrip");
        let (mut storage, _) = reopen(&dir);
        storage.save_vote(Term(3), Some(NodeId(2))).unwrap();
        storage
            .write_log(Index(0), &[entry(1, 10), entry(1, 11), entry(2, 12)])
            .unwrap();
        // Replace everything after the first entry, then add one more.
        storage.write_log(Index(1), &[entry(3, 20)]).unwrap();
        storage.write_log(Index(2), &[entry(3, 21)]).unwrap();
        storage.write_log(Index(3), &[]).unwrap();
        drop(storage);

        let (mut storage, durable) = reopen(&dir);
        let log = vec![entry(1, 10), entry(3, 20), entry(3, 21)];
        assert_eq!(durable, Durable::new(Term(3), Some(NodeId(2)), log.clone()));
        // And it carries on from there.
        storage.write_log(Index(2), &[entry(4, 30)]).unwrap();
        storage.save_vote(Term(4), None).unwrap();
        drop(storage);
        let (_, durable) = reopen(&dir);
        assert_eq!(
            durable,
            Durable::new(
                Term(4),
                None,
                vec![log[0].clone(), log[1].clone(), entry(4, 30)]
            )
        );
    }

    #[test]
    fn a_line_cut_short_by_a_crash_is_removed() {
        let dir = TempDir::new("torn");
        let (mut storage, _) = reopen(&dir);
        storage
            .write_log(Index(0), &[entry(1, 1), entry(1, 2)])
            .unwrap();
        drop(storage);
        let path = dir.0.join("log");
        let whole = fs::read(&path).unwrap();
        let mut torn = whole.clone();
        torn.extend_from_slice(&line("{\"term\":1,\"payl").as_bytes()[..20]);
        fs::write(&path, &torn).unwrap();

        let (mut storage, durable) = reopen(&dir);
        assert_eq!(durable.log, [entry(1, 1), entry(1, 2)]);
        assert_eq!(fs::read(&path).unwrap(), whole, "the file is repaired");
        storage.write_log(Index(2), &[entry(1, 3)]).unwrap();
        drop(storage);
        assert_eq!(reopen(&dir).1.log.len(), 3);
    }

    #[test]
    fn a_damaged_file_is_an_error() {
        let dir = TempDir::new("damaged");
        let (mut storage, _) = reopen(&dir);
        storage.save_vote(Term(1), None).unwrap();
        storage
            .write_log(Index(0), &[entry(1, 1), entry(1, 2)])
            .unwrap();
        drop(storage);

        let path = dir.0.join("log");
        let good = fs::read(&path).unwrap();
        let mut bad = good.clone();
        let flip = bad.len() / 4;
        bad[flip] ^= 1;
        fs::write(&path, &bad).unwrap();
        let error = FileStorage::<u32>::open(&dir.0)
            .unwrap()
            .load()
            .unwrap_err();
        assert!(error.to_string().contains("entry 1 is damaged"), "{error}");

        fs::write(&path, &good).unwrap();
        fs::write(
            dir.0.join("vote"),
            "0000000000000000 {\"term\":9,\"voted_for\":null}\n",
        )
        .unwrap();
        let error = FileStorage::<u32>::open(&dir.0)
            .unwrap()
            .load()
            .unwrap_err();
        assert!(error.to_string().contains("vote is damaged"), "{error}");
    }

    #[test]
    fn a_directory_is_used_by_one_process_at_a_time() {
        let dir = TempDir::new("lock");
        let (first, _) = reopen(&dir);
        let error = FileStorage::<u32>::open(&dir.0).unwrap_err();
        assert!(error.to_string().contains("in use"), "{error}");
        drop(first);
        assert!(FileStorage::<u32>::open(&dir.0).is_ok());
    }
}
